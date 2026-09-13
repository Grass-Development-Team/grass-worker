use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use grass_cache::Cache;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    domain::external_login::{AuthorizationFlow, configured_site_url, flow_key, provider_by_slug},
    infra::{
        database::entity::IdentityProviderKind, error::AppError, http::redirects::safe_return_to,
    },
    state::ControlApiState,
};
use std::time::Duration as StdDuration;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/providers/{slug}/start", axum::routing::get(start))
}

const FLOW_TTL: StdDuration = StdDuration::from_secs(10 * 60);

const STATE_COOKIE: &str = "oauth_state";

#[derive(Deserialize)]
pub struct StartQuery {
    pub return_to: Option<String>,
    pub registration_code: Option<String>,
}

impl StartQuery {
    fn registration_code(&self) -> Option<&str> {
        self.registration_code
            .as_deref()
            .filter(|code| !code.trim().is_empty())
    }
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
                registration_code: query.registration_code().map(str::to_owned),
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
    let (cookie_secure, development_enabled) = {
        let config = state.config.read().unwrap();
        (config.session.cookie_secure, config.development_enabled())
    };
    let state_cookie = state_cookie(
        &state_token,
        apple_form_post,
        cookie_secure,
        development_enabled,
    );
    Ok((
        jar.add(state_cookie),
        Redirect::temporary(authorization_url.as_str()),
    ))
}

fn state_cookie(
    state: &str,
    apple_form_post: bool,
    configured_secure: bool,
    development_enabled: bool,
) -> Cookie<'static> {
    let mut cookie = Cookie::new(STATE_COOKIE, grass_token::hash_token(state));
    cookie.set_path("/api/v1/auth/providers");
    cookie.set_http_only(true);
    cookie.set_max_age(time::Duration::minutes(10));
    cookie.set_same_site(if apple_form_post {
        SameSite::None
    } else {
        SameSite::Lax
    });
    cookie.set_secure((apple_form_post || configured_secure) && !development_enabled);
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum_extra::extract::cookie::SameSite;
    use uuid::Uuid;
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
    fn oauth_state_cookie_matches_the_callback_transport() {
        let regular = state_cookie("state", false, true, false);
        assert_eq!(regular.name(), STATE_COOKIE);
        assert_eq!(regular.path(), Some("/api/v1/auth/providers"));
        assert_eq!(regular.http_only(), Some(true));
        assert_eq!(regular.same_site(), Some(SameSite::Lax));
        assert_eq!(regular.secure(), Some(true));

        let regular_without_configured_secure = state_cookie("state", false, false, false);
        assert_eq!(
            regular_without_configured_secure.same_site(),
            Some(SameSite::Lax)
        );
        assert_eq!(regular_without_configured_secure.secure(), Some(false));

        let development = state_cookie("state", false, true, true);
        assert_eq!(development.same_site(), Some(SameSite::Lax));
        assert_eq!(development.secure(), Some(false));

        let apple = state_cookie("state", true, true, false);
        assert_eq!(apple.same_site(), Some(SameSite::None));
        assert_eq!(apple.secure(), Some(true));

        let apple_without_configured_secure = state_cookie("state", true, false, false);
        assert_eq!(
            apple_without_configured_secure.same_site(),
            Some(SameSite::None)
        );
        assert_eq!(apple_without_configured_secure.secure(), Some(true));

        let apple_development = state_cookie("state", true, true, true);
        assert_eq!(apple_development.same_site(), Some(SameSite::None));
        assert_eq!(apple_development.secure(), Some(false));
    }
}
