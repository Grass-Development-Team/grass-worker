pub(crate) mod by_code_id;

use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::codes::{self, CodeScope, CodeStatus},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, code, user},
        error::{AppError, ok_response},
        http::{extractors::Session, timestamps::ts},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/codes", axum::routing::get(list).post(generate))
        .merge(by_code_id::router())
}

#[derive(Deserialize)]
struct GenerateCodesRequest {
    scope: String,
    count: usize,
    expires_in_days: Option<i64>,
    #[serde(default)]
    never_expires: bool,
}

struct GenerationInput {
    scope: CodeScope,
    count: usize,
    expires_at: Option<OffsetDateTime>,
}

fn validate_generation(
    request: GenerateCodesRequest,
    now: OffsetDateTime,
) -> Result<GenerationInput, AppError> {
    const OP: &str = "admin.codes.generate.validate";
    let scope = CodeScope::parse(request.scope.trim()).ok_or_else(|| AppError::Validation {
        op: OP,
        message: "code scope is not registered".to_owned(),
    })?;
    if !(1..=500).contains(&request.count) {
        return Err(AppError::Validation {
            op: OP,
            message: "code count must be between 1 and 500".to_owned(),
        });
    }
    let expires_at = if request.never_expires {
        None
    } else {
        let days = request.expires_in_days.unwrap_or(30);
        if !(1..=3650).contains(&days) {
            return Err(AppError::Validation {
                op: OP,
                message: "expiration must be between 1 and 3650 days".to_owned(),
            });
        }
        Some(now + Duration::days(days))
    };
    Ok(GenerationInput {
        scope,
        count: request.count,
        expires_at,
    })
}

#[derive(Deserialize)]
struct ListCodesQuery {
    scope: Option<String>,
    status: Option<String>,
    page: Option<u64>,
    per_page: Option<u64>,
}

#[derive(Serialize)]
struct CodeUserView {
    id: Uuid,
    email: String,
    display_name: Option<String>,
}

#[derive(Serialize)]
struct CodeView {
    id: Uuid,
    code: String,
    scope: String,
    status: &'static str,
    expires_at: serde_json::Value,
    used_at: serde_json::Value,
    used_by: Option<CodeUserView>,
    revoked_at: serde_json::Value,
    created_at: serde_json::Value,
}

fn code_view(item: &code::Model, used_by: Option<&user::Model>, now: OffsetDateTime) -> CodeView {
    CodeView {
        id: item.id,
        code: format!("{}...{}", item.token_prefix, item.token_suffix),
        scope: item.scope.clone(),
        status: codes::lifecycle_status(item.used_at, item.revoked_at, item.expires_at, now)
            .as_str(),
        expires_at: ts(item.expires_at),
        used_at: ts(item.used_at),
        used_by: used_by.map(|user| CodeUserView {
            id: user.id,
            email: user.email.clone(),
            display_name: user.display_name.clone(),
        }),
        revoked_at: ts(item.revoked_at),
        created_at: ts(item.created_at),
    }
}

async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListCodesQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.codes.list";
    let db = crate::infra::http::database(&state, OP)?;
    let scope = query
        .scope
        .as_deref()
        .map(|scope| {
            CodeScope::parse(scope).ok_or_else(|| AppError::Validation {
                op: OP,
                message: "code scope is not registered".to_owned(),
            })
        })
        .transpose()?;
    let status = query
        .status
        .as_deref()
        .map(|status| {
            CodeStatus::parse(status).ok_or_else(|| AppError::Validation {
                op: OP,
                message: "code status is invalid".to_owned(),
            })
        })
        .transpose()?;
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(50).clamp(1, 100);
    let now = OffsetDateTime::now_utc();
    let paginator = codes::query_codes(scope, status, now).paginate(db, per_page);
    let total = paginator
        .num_items()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let items =
        paginator
            .fetch_page(page - 1)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
    let used_by_ids = items
        .iter()
        .filter_map(|item| item.used_by_user_id)
        .collect::<Vec<_>>();
    let users = if used_by_ids.is_empty() {
        Vec::new()
    } else {
        user::Entity::find()
            .filter(user::Column::Id.is_in(used_by_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
    };
    let users = users
        .into_iter()
        .map(|user| (user.id, user))
        .collect::<HashMap<_, _>>();

    Ok(ok_response(ListResponse {
        scopes: codes::registered_scopes()
            .iter()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>(),
        codes: items
            .iter()
            .map(|item| {
                code_view(
                    item,
                    item.used_by_user_id.and_then(|id| users.get(&id)),
                    now,
                )
            })
            .collect::<Vec<_>>(),
        pagination: ListPaginationResponse {
            page,
            per_page,
            total,
            total_pages: total.div_ceil(per_page),
        },
    }))
}

async fn generate(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<GenerateCodesRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.codes.generate";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
    let input = validate_generation(body, OffsetDateTime::now_utc())?;
    let generated = codes::generate_codes(
        db,
        input.scope,
        input.count,
        input.expires_at,
        Some(data.user_id),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "code.generated".to_owned(),
            target_type: "code".to_owned(),
            target_id: None,
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "scope": input.scope.as_str(),
                "count": input.count,
                "expires_at": ts(input.expires_at),
            }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(GenerateResponse {
        codes: generated
            .iter()
            .map(|item| GenerateCodesResponse {
                id: item.model.id,
                code: item.value.clone(),
                scope: item.model.scope.clone(),
                expires_at: item.model.expires_at,
                created_at: item.model.created_at,
            })
            .collect::<Vec<_>>(),
    }))
}

#[derive(serde::Serialize)]
struct ListPaginationResponse {
    page: u64,
    per_page: u64,
    total: u64,
    total_pages: u64,
}

#[derive(serde::Serialize)]
struct ListResponse {
    scopes: Vec<&'static str>,
    codes: Vec<CodeView>,
    pagination: ListPaginationResponse,
}

#[derive(serde::Serialize)]
struct GenerateCodesResponse {
    id: uuid::Uuid,
    code: String,
    scope: String,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    expires_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct GenerateResponse {
    codes: Vec<GenerateCodesResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::codes;
    use time::Duration;
    use time::OffsetDateTime;

    #[test]
    fn generation_defaults_to_thirty_days_and_validates_limits() {
        let now = OffsetDateTime::now_utc();
        let request = GenerateCodesRequest {
            scope: "registration".to_owned(),
            count: 25,
            expires_in_days: None,
            never_expires: false,
        };
        let input = validate_generation(request, now).unwrap();

        assert_eq!(input.scope, codes::CodeScope::Registration);
        assert_eq!(input.count, 25);
        assert_eq!(input.expires_at, Some(now + Duration::days(30)));

        assert!(
            validate_generation(
                GenerateCodesRequest {
                    scope: "registration".to_owned(),
                    count: 0,
                    expires_in_days: None,
                    never_expires: false,
                },
                now,
            )
            .is_err()
        );
        assert!(
            validate_generation(
                GenerateCodesRequest {
                    scope: "registration".to_owned(),
                    count: 501,
                    expires_in_days: None,
                    never_expires: false,
                },
                now,
            )
            .is_err()
        );
        assert!(
            validate_generation(
                GenerateCodesRequest {
                    scope: "discount".to_owned(),
                    count: 1,
                    expires_in_days: None,
                    never_expires: false,
                },
                now,
            )
            .is_err()
        );
    }

    #[test]
    fn generation_can_create_codes_without_expiration() {
        let input = validate_generation(
            GenerateCodesRequest {
                scope: "registration".to_owned(),
                count: 1,
                expires_in_days: Some(90),
                never_expires: true,
            },
            OffsetDateTime::now_utc(),
        )
        .unwrap();

        assert_eq!(input.expires_at, None);
    }
}
