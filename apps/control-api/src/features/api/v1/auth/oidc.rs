use std::time::Duration as StdDuration;

use axum::{
    Form,
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use grass_cache::Cache;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::{authentication, registration, settings, teams, users},
    infra::{
        database::entity::{
            IdentityProviderKind, PlatformRole, TeamKind, UserStatus, auth_identity_provider, user,
            user_external_identity,
        },
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

const FLOW_TTL: StdDuration = StdDuration::from_secs(10 * 60);
const STATE_COOKIE: &str = "oauth_state";

#[derive(Debug, Deserialize, Serialize)]
struct AuthorizationFlow {
    provider_id: Uuid,
    nonce: String,
    pkce_verifier: String,
    return_to: String,
    registration_code: Option<String>,
    redirect_uri: String,
}

#[derive(Deserialize)]
pub struct StartQuery {
    pub return_to: Option<String>,
    pub registration_code: Option<String>,
}

pub async fn providers(
    State(state): State<ControlApiState>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.providers.list";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let providers = auth_identity_provider::Entity::find()
        .filter(auth_identity_provider::Column::Enabled.eq(true))
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let signup_policy = settings::get_setting(db, "signup.policy")
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let signup_policy = registration::SignupPolicy::parse(
        signup_policy
            .as_ref()
            .and_then(|setting| setting.value.as_str()),
    )
    .map_err(|error| AppError::Internal {
        op: OP,
        message: error.to_string(),
    })?;
    Ok(ok_response(json!({
        "providers": providers.iter().map(|provider| json!({
            "slug": provider.slug,
            "name": provider.name,
            "kind": provider.kind.as_str(),
        })).collect::<Vec<_>>(),
        "password_recovery_available": state.config.read().unwrap().mail.enabled(),
        "signup_policy": signup_policy.as_str(),
        "registration_email_verification": authentication::registration_verification_required(db)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?,
        "password_policy": authentication::password_policy(db)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?,
    })))
}

pub async fn start(
    State(state): State<ControlApiState>,
    Path(slug): Path<String>,
    jar: CookieJar,
    Query(query): Query<StartQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.providers.start";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let provider = provider_by_slug(db, &slug, OP).await?;
    let site_url = configured_site_url(db, OP).await?;
    let redirect_uri = format!(
        "{}/api/v1/auth/providers/{}/callback",
        site_url.trim_end_matches('/'),
        provider.slug
    );
    let state_token = grass_token::generate_token();
    let nonce = grass_token::generate_token();
    let pkce_verifier = grass_token::generate_token();
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce_verifier.as_bytes()));
    let return_to = safe_return_to(query.return_to.as_deref());
    cache
        .set(
            &flow_key(&state_token),
            &serde_json::to_string(&AuthorizationFlow {
                provider_id: provider.id,
                nonce: nonce.clone(),
                pkce_verifier,
                return_to,
                registration_code: query
                    .registration_code
                    .filter(|code| !code.trim().is_empty()),
                redirect_uri: redirect_uri.clone(),
            })
            .map_err(|error| AppError::Internal {
                op: OP,
                message: error.to_string(),
            })?,
            FLOW_TTL,
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let mut authorization_url =
        url::Url::parse(&provider.authorization_url).map_err(|_| AppError::Internal {
            op: OP,
            message: "identity provider authorization URL is invalid".to_owned(),
        })?;
    let scopes = provider
        .scopes
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|scope| scope.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    authorization_url
        .query_pairs_mut()
        .append_pair("client_id", &provider.client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scopes)
        .append_pair("state", &state_token)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256");
    if matches!(provider.kind, IdentityProviderKind::Oidc) {
        authorization_url
            .query_pairs_mut()
            .append_pair("nonce", &nonce);
        if provider.issuer_url.as_deref() == Some("https://appleid.apple.com") {
            authorization_url
                .query_pairs_mut()
                .append_pair("response_mode", "form_post");
        }
    }
    let apple_form_post = provider.issuer_url.as_deref() == Some("https://appleid.apple.com");
    let state_cookie = state_cookie(
        &state_token,
        apple_form_post,
        state.config.read().unwrap().session.cookie_secure,
    );
    Ok((
        jar.add(state_cookie),
        Redirect::temporary(authorization_url.as_str()),
    ))
}

#[derive(Clone, Deserialize)]
pub struct CallbackPayload {
    pub code: Option<String>,
    pub state: String,
    pub error: Option<String>,
}

pub async fn callback(
    State(state): State<ControlApiState>,
    Path(slug): Path<String>,
    jar: CookieJar,
    Query(payload): Query<CallbackPayload>,
) -> Result<Response, AppError> {
    callback_core(state, slug, jar, payload).await
}

pub async fn callback_form(
    State(state): State<ControlApiState>,
    Path(slug): Path<String>,
    jar: CookieJar,
    Form(payload): Form<CallbackPayload>,
) -> Result<Response, AppError> {
    callback_core(state, slug, jar, payload).await
}

async fn callback_core(
    state: ControlApiState,
    slug: String,
    jar: CookieJar,
    payload: CallbackPayload,
) -> Result<Response, AppError> {
    const OP: &str = "auth.providers.callback";
    if payload.error.is_some() {
        return Err(AppError::Unauthorized {
            op: OP,
            message: "identity provider authorization was denied".to_owned(),
        });
    }
    let state_cookie = jar
        .get(STATE_COOKIE)
        .filter(|cookie| cookie.value() == grass_token::hash_token(&payload.state))
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "authorization state is not bound to this browser".to_owned(),
        })?;
    let mut clear_state_cookie = Cookie::new(STATE_COOKIE, state_cookie.value().to_owned());
    clear_state_cookie.set_path("/api/v1/auth/providers");
    clear_state_cookie.make_removal();
    let jar = jar.add(clear_state_cookie);
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let flow: AuthorizationFlow = cache
        .take(&flow_key(&payload.state))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .and_then(|flow| serde_json::from_str(&flow).ok())
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "authorization state is invalid or expired".to_owned(),
        })?;
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let provider = provider_by_slug(db, &slug, OP).await?;
    if provider.id != flow.provider_id {
        return Err(AppError::Unauthorized {
            op: OP,
            message: "authorization state does not match the provider".to_owned(),
        });
    }
    let code = payload.code.ok_or_else(|| AppError::Unauthorized {
        op: OP,
        message: "identity provider did not return an authorization code".to_owned(),
    })?;
    let client_secret = decrypt_client_secret(&state, &provider, OP)?;
    let token = exchange_code(&provider, &client_secret, &code, &flow, OP).await?;
    let identity = match provider.kind {
        IdentityProviderKind::Oidc => oidc_identity(&provider, &token, &flow.nonce, OP).await?,
        IdentityProviderKind::Github => github_identity(&provider, &token.access_token, OP).await?,
    };
    let user = resolve_user(
        db,
        &provider,
        identity,
        flow.registration_code.as_deref(),
        OP,
    )
    .await?;
    if !matches!(user.status, UserStatus::Active) {
        return Err(AppError::Forbidden {
            op: OP,
            message: "user account is disabled".to_owned(),
        });
    }
    let site_url = configured_site_url(db, OP).await?;
    if let Some(payload) =
        super::mfa::begin_login_payload(&state, &user, Some(&flow.return_to)).await?
    {
        let challenge = payload
            .get("challenge_token")
            .and_then(|value| value.as_str())
            .ok_or_else(|| AppError::Internal {
                op: OP,
                message: "MFA challenge response is incomplete".to_owned(),
            })?;
        let destination = format!(
            "{}/mfa#challenge={}",
            site_url.trim_end_matches('/'),
            url::form_urlencoded::byte_serialize(challenge.as_bytes()).collect::<String>()
        );
        return Ok((jar, Redirect::to(&destination)).into_response());
    }
    let (jar, _) = super::login::create_authenticated_session(&state, cache, jar, &user).await?;
    let destination = format!("{}{}", site_url.trim_end_matches('/'), flow.return_to);
    Ok((jar, Redirect::to(&destination)).into_response())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    id_token: Option<String>,
}

async fn exchange_code(
    provider: &auth_identity_provider::Model,
    client_secret: &str,
    code: &str,
    flow: &AuthorizationFlow,
    op: &'static str,
) -> Result<TokenResponse, AppError> {
    reqwest::Client::new()
        .post(&provider.token_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", provider.client_id.as_str()),
            ("client_secret", client_secret),
            ("code", code),
            ("redirect_uri", flow.redirect_uri.as_str()),
            ("code_verifier", flow.pkce_verifier.as_str()),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| AppError::Unauthorized {
            op,
            message: "identity provider token exchange failed".to_owned(),
        })?
        .json()
        .await
        .map_err(|_| AppError::Unauthorized {
            op,
            message: "identity provider token response is invalid".to_owned(),
        })
}

struct ExternalIdentity {
    subject: String,
    email: Option<String>,
    email_verified: bool,
    display_name: Option<String>,
}

async fn oidc_identity(
    provider: &auth_identity_provider::Model,
    token: &TokenResponse,
    nonce: &str,
    op: &'static str,
) -> Result<ExternalIdentity, AppError> {
    let id_token = token
        .id_token
        .as_deref()
        .ok_or_else(|| AppError::Unauthorized {
            op,
            message: "OIDC provider did not return an ID token".to_owned(),
        })?;
    let header = decode_header(id_token).map_err(|_| AppError::Unauthorized {
        op,
        message: "OIDC ID token header is invalid".to_owned(),
    })?;
    if matches!(
        header.alg,
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
    ) {
        return Err(AppError::Unauthorized {
            op,
            message: "OIDC ID token uses an unsupported signing algorithm".to_owned(),
        });
    }
    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| AppError::Unauthorized {
            op,
            message: "OIDC ID token has no key identifier".to_owned(),
        })?;
    let jwks: JwkSet = reqwest::Client::new()
        .get(provider.jwks_url.as_deref().unwrap_or_default())
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| AppError::Unauthorized {
            op,
            message: "OIDC signing keys could not be loaded".to_owned(),
        })?
        .json()
        .await
        .map_err(|_| AppError::Unauthorized {
            op,
            message: "OIDC signing keys are invalid".to_owned(),
        })?;
    let key = jwks.find(kid).ok_or_else(|| AppError::Unauthorized {
        op,
        message: "OIDC signing key was not found".to_owned(),
    })?;
    let mut validation = Validation::new(header.alg);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.set_issuer(&[provider.issuer_url.as_deref().unwrap_or_default()]);
    validation.set_audience(&[provider.client_id.as_str()]);
    let claims = decode::<serde_json::Value>(
        id_token,
        &DecodingKey::from_jwk(key).map_err(|_| AppError::Unauthorized {
            op,
            message: "OIDC signing key is unsupported".to_owned(),
        })?,
        &validation,
    )
    .map_err(|_| AppError::Unauthorized {
        op,
        message: "OIDC ID token validation failed".to_owned(),
    })?
    .claims;
    if claims.get("nonce").and_then(|value| value.as_str()) != Some(nonce) {
        return Err(AppError::Unauthorized {
            op,
            message: "OIDC nonce validation failed".to_owned(),
        });
    }
    let subject = claims
        .get("sub")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned();
    let mut email = claims
        .get("email")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let mut email_verified = claims.get("email_verified").is_some_and(claim_is_true);
    let mut display_name = claims
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    if (!email_verified || email.is_none())
        && let Some(userinfo_url) = provider.userinfo_url.as_deref()
    {
        let userinfo: serde_json::Value = reqwest::Client::new()
            .get(userinfo_url)
            .bearer_auth(&token.access_token)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| AppError::Unauthorized {
                op,
                message: "OIDC user information could not be loaded".to_owned(),
            })?
            .json()
            .await
            .map_err(|_| AppError::Unauthorized {
                op,
                message: "OIDC user information is invalid".to_owned(),
            })?;
        if userinfo.get("sub").and_then(|value| value.as_str()) != Some(subject.as_str()) {
            return Err(AppError::Unauthorized {
                op,
                message: "OIDC user information subject does not match".to_owned(),
            });
        }
        let userinfo_email = userinfo
            .get("email")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        let userinfo_email_verified = userinfo.get("email_verified").is_some_and(claim_is_true);
        if let Some(userinfo_email) = userinfo_email {
            if email.as_deref() == Some(userinfo_email.as_str()) {
                email_verified = email_verified || userinfo_email_verified;
            } else {
                email = Some(userinfo_email);
                email_verified = userinfo_email_verified;
            }
        }
        display_name = display_name.or_else(|| {
            userinfo
                .get("name")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        });
    }
    Ok(ExternalIdentity {
        subject,
        email,
        email_verified,
        display_name,
    })
}

fn claim_is_true(value: &serde_json::Value) -> bool {
    value == true || value.as_str() == Some("true")
}

async fn github_identity(
    provider: &auth_identity_provider::Model,
    access_token: &str,
    op: &'static str,
) -> Result<ExternalIdentity, AppError> {
    let client = reqwest::Client::new();
    let userinfo_url = provider.userinfo_url.as_deref().unwrap_or_default();
    let profile: serde_json::Value = client
        .get(userinfo_url)
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "grass-worker")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| AppError::Unauthorized {
            op,
            message: "GitHub profile could not be loaded".to_owned(),
        })?
        .json()
        .await
        .map_err(|_| AppError::Unauthorized {
            op,
            message: "GitHub profile is invalid".to_owned(),
        })?;
    let emails_url = format!("{}/emails", userinfo_url.trim_end_matches('/'));
    let emails: Vec<serde_json::Value> = client
        .get(emails_url)
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "grass-worker")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| AppError::Unauthorized {
            op,
            message: "GitHub verified email could not be loaded".to_owned(),
        })?
        .json()
        .await
        .map_err(|_| AppError::Unauthorized {
            op,
            message: "GitHub email response is invalid".to_owned(),
        })?;
    let email = emails
        .iter()
        .find(|email| {
            email.get("primary").and_then(|value| value.as_bool()) == Some(true)
                && email.get("verified").and_then(|value| value.as_bool()) == Some(true)
        })
        .or_else(|| {
            emails
                .iter()
                .find(|email| email.get("verified").and_then(|value| value.as_bool()) == Some(true))
        })
        .and_then(|email| email.get("email"))
        .and_then(|email| email.as_str())
        .map(str::to_owned);
    Ok(ExternalIdentity {
        subject: profile
            .get("id")
            .map(ToString::to_string)
            .unwrap_or_default(),
        email,
        email_verified: true,
        display_name: profile
            .get("name")
            .and_then(|value| value.as_str())
            .or_else(|| profile.get("login").and_then(|value| value.as_str()))
            .map(str::to_owned),
    })
}

async fn resolve_user(
    db: &sea_orm::DatabaseConnection,
    provider: &auth_identity_provider::Model,
    identity: ExternalIdentity,
    registration_code: Option<&str>,
    op: &'static str,
) -> Result<user::Model, AppError> {
    if identity.subject.trim().is_empty() {
        return Err(AppError::Unauthorized {
            op,
            message: "identity provider returned no subject".to_owned(),
        });
    }
    if let Some(link) = user_external_identity::Entity::find()
        .filter(user_external_identity::Column::ProviderId.eq(provider.id))
        .filter(user_external_identity::Column::Subject.eq(&identity.subject))
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
    {
        return users::get_user_by_id(db, link.user_id)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?
            .ok_or_else(|| AppError::NotFound {
                op,
                message: "linked user was not found".to_owned(),
            });
    }
    if !identity.email_verified {
        return Err(AppError::Forbidden {
            op,
            message: "identity provider email is not verified".to_owned(),
        });
    }
    let email = grass_validator::normalize_email(identity.email.as_deref().unwrap_or_default())
        .map_err(|_| AppError::Forbidden {
            op,
            message: "identity provider returned no usable email".to_owned(),
        })?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let user = if let Some(user) = users::get_user_by_email(&transaction, &email)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    {
        if user.email_verified_at.is_none() {
            let mut active: user::ActiveModel = user.into();
            active.email_verified_at = Set(Some(time::OffsetDateTime::now_utc()));
            active
                .update(&transaction)
                .await
                .map_err(|source| AppError::Infrastructure {
                    op,
                    source: source.into(),
                })?
        } else {
            user
        }
    } else {
        let signup_policy = settings::get_setting(&transaction, "signup.policy")
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?
            .and_then(|setting| setting.value.as_str().map(str::to_owned));
        let signup_policy = registration::SignupPolicy::parse(signup_policy.as_deref())
            .map_err(|error| super::register::map_registration_access_error(error, op))?;
        let registration_grant = registration::authorize_registration(
            &transaction,
            signup_policy,
            &email,
            registration_code,
        )
        .await
        .map_err(|error| super::register::map_registration_access_error(error, op))?;
        let user = users::create_user(
            &transaction,
            users::CreateUserParams {
                email: email.clone(),
                display_name: identity.display_name.clone(),
                password_hash: None,
                platform_role: PlatformRole::User,
                email_verified_at: Some(time::OffsetDateTime::now_utc()),
            },
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
        let slug = format!(
            "{}-{}",
            super::register::personal_team_slug(&email),
            &user.id.simple().to_string()[..8]
        );
        teams::create_team_with_connection(
            &transaction,
            teams::CreateTeamParams {
                slug,
                name: format!(
                    "{}'s Team",
                    identity.display_name.as_deref().unwrap_or("User")
                ),
                kind: TeamKind::Personal,
                owner_user_id: user.id,
                group_id: None,
            },
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
        registration::consume_registration_grant(&transaction, registration_grant, user.id)
            .await
            .map_err(|error| super::register::map_registration_access_error(error, op))?;
        user
    };
    let now = time::OffsetDateTime::now_utc();
    user_external_identity::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user.id),
        provider_id: Set(provider.id),
        subject: Set(identity.subject),
        email: Set(email),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&transaction)
    .await
    .map_err(|source| AppError::Infrastructure {
        op,
        source: source.into(),
    })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    Ok(user)
}

async fn provider_by_slug(
    db: &sea_orm::DatabaseConnection,
    slug: &str,
    op: &'static str,
) -> Result<auth_identity_provider::Model, AppError> {
    auth_identity_provider::Entity::find()
        .filter(auth_identity_provider::Column::Slug.eq(slug))
        .filter(auth_identity_provider::Column::Enabled.eq(true))
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "identity provider not found".to_owned(),
        })
}

fn decrypt_client_secret(
    state: &ControlApiState,
    provider: &auth_identity_provider::Model,
    op: &'static str,
) -> Result<String, AppError> {
    let envelope: grass_crypto::AeadEnvelope =
        serde_json::from_value(provider.client_secret_envelope.clone()).map_err(|_| {
            AppError::Internal {
                op,
                message: "identity provider secret envelope is invalid".to_owned(),
            }
        })?;
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let plaintext = grass_crypto::decrypt_secret(
        &envelope,
        &authentication::authentication_key(&secret),
        format!("grass-identity-provider:v1:{}", provider.id).as_bytes(),
    )
    .map_err(|_| AppError::Internal {
        op,
        message: "identity provider secret could not be decrypted".to_owned(),
    })?;
    String::from_utf8(plaintext).map_err(|_| AppError::Internal {
        op,
        message: "identity provider secret is invalid".to_owned(),
    })
}

async fn configured_site_url(
    db: &sea_orm::DatabaseConnection,
    op: &'static str,
) -> Result<String, AppError> {
    settings::get_setting(db, "site.url")
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .and_then(|setting| setting.value.as_str().map(str::to_owned))
        .ok_or_else(|| AppError::Internal {
            op,
            message: "site.url is not configured".to_owned(),
        })
}

fn flow_key(state: &str) -> String {
    format!("auth:oauth:flow:{}", grass_token::hash_token(state))
}

fn state_cookie(state: &str, apple_form_post: bool, cookie_secure: bool) -> Cookie<'static> {
    let mut cookie = Cookie::new(STATE_COOKIE, grass_token::hash_token(state));
    cookie.set_path("/api/v1/auth/providers");
    cookie.set_http_only(true);
    cookie.set_max_age(time::Duration::minutes(10));
    cookie.set_same_site(if apple_form_post {
        SameSite::None
    } else {
        SameSite::Lax
    });
    cookie.set_secure(apple_form_post || cookie_secure);
    cookie
}

pub(crate) fn safe_return_to(value: Option<&str>) -> String {
    value
        .filter(|value| {
            value.starts_with('/')
                && !value.starts_with("//")
                && !value.contains('\\')
                && value.len() <= 4096
                && !value.chars().any(char::is_control)
        })
        .unwrap_or("/")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_flow_keeps_the_registration_code() {
        let flow = AuthorizationFlow {
            provider_id: Uuid::nil(),
            nonce: "nonce".to_owned(),
            pkce_verifier: "verifier".to_owned(),
            return_to: "/".to_owned(),
            registration_code: Some("registration-code".to_owned()),
            redirect_uri: "https://example.com/callback".to_owned(),
        };

        let serialized = serde_json::to_string(&flow).unwrap();
        let restored: AuthorizationFlow = serde_json::from_str(&serialized).unwrap();

        assert_eq!(
            restored.registration_code.as_deref(),
            Some("registration-code")
        );
    }

    #[test]
    fn return_destinations_must_remain_local() {
        assert_eq!(safe_return_to(Some("/projects")), "/projects");
        assert_eq!(
            safe_return_to(Some("/invitations/accept?token=invite-token")),
            "/invitations/accept?token=invite-token"
        );
        assert_eq!(safe_return_to(Some("//example.com")), "/");
        assert_eq!(safe_return_to(Some("https://example.com")), "/");
        assert_eq!(safe_return_to(Some("/projects\nnext")), "/");
    }

    #[test]
    fn oidc_boolean_claims_accept_only_boolean_or_literal_true() {
        assert!(claim_is_true(&serde_json::json!(true)));
        assert!(claim_is_true(&serde_json::json!("true")));
        assert!(!claim_is_true(&serde_json::json!(1)));
        assert!(!claim_is_true(&serde_json::json!("TRUE")));
    }

    #[test]
    fn oauth_state_cookie_matches_the_callback_transport() {
        let regular = state_cookie("state", false, false);
        assert_eq!(regular.name(), STATE_COOKIE);
        assert_eq!(regular.path(), Some("/api/v1/auth/providers"));
        assert_eq!(regular.http_only(), Some(true));
        assert_eq!(regular.same_site(), Some(SameSite::Lax));
        assert_eq!(regular.secure(), Some(false));

        let apple = state_cookie("state", true, false);
        assert_eq!(apple.same_site(), Some(SameSite::None));
        assert_eq!(apple.secure(), Some(true));
    }
}
