use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::codes::{self, CodeUseError},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, code, user},
        error::{AppError, ok_response},
        http::{extractors::Session, timestamps::ts},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/codes/{code_id}/revoke", axum::routing::post(revoke))
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

fn map_code_error(error: CodeUseError, op: &'static str) -> AppError {
    match error {
        CodeUseError::NotFound => AppError::NotFound {
            op,
            message: "code not found".to_owned(),
        },
        CodeUseError::WrongScope => AppError::Conflict {
            op,
            message: "code scope is not registered".to_owned(),
        },
        CodeUseError::Used => AppError::Conflict {
            op,
            message: "code has already been used".to_owned(),
        },
        CodeUseError::Expired => AppError::Gone {
            op,
            message: "code has expired".to_owned(),
        },
        CodeUseError::Revoked => AppError::Conflict {
            op,
            message: "code has been revoked".to_owned(),
        },
        CodeUseError::Database(source) => AppError::Infrastructure { op, source },
    }
}

async fn revoke(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(code_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.codes.revoke";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
    let item = codes::revoke_code(db, code_id)
        .await
        .map_err(|error| map_code_error(error, OP))?;
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "code.revoked".to_owned(),
            target_type: "code".to_owned(),
            target_id: Some(item.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "scope": item.scope }),
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

    Ok(ok_response(RevokeResponse {
        code: code_view(&item, None, OffsetDateTime::now_utc()),
    }))
}

#[derive(serde::Serialize)]
struct RevokeResponse {
    code: CodeView,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::codes;
    use time::OffsetDateTime;

    #[test]
    fn list_view_exposes_only_the_stored_preview() {
        let generated = codes::prepare_code(codes::CodeScope::Registration, None, None);
        let view = code_view(&generated.model, None, OffsetDateTime::now_utc());
        let json = serde_json::to_value(view).unwrap();

        assert_eq!(
            json["code"],
            format!("{}...{}", &generated.value[..6], &generated.value[36..])
        );
        assert!(json.get("token_hash").is_none());
        assert!(!json.to_string().contains(&generated.value));
        assert_eq!(json["scope"], "registration");
        assert_eq!(json["status"], "available");
    }
}
