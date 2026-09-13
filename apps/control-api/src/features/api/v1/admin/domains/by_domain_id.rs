pub(crate) mod approve;
pub(crate) mod reject;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::host_bindings::{DeleteHostScope, HostBindingService},
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/domains/{domain_id}", axum::routing::delete(remove))
        .merge(approve::router())
        .merge(reject::router())
}

#[derive(Default, Deserialize)]
pub struct DomainReason {
    #[serde(default)]
    pub reason: Option<String>,
}

fn optional_reason(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

/// DELETE /api/v1/admin/domains/{domain_id}
pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(domain_id): Path<Uuid>,
    body: Option<Json<DomainReason>>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.domains.delete";
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;
    let reason = optional_reason(body.and_then(|Json(body)| body.reason));
    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    HostBindingService::new(db, cache, &platform_secret)
        .delete_host(
            OP,
            domain_id,
            DeleteHostScope::Platform {
                actor_user_id: data.user_id,
                reason: reason.clone(),
            },
        )
        .await?;
    Ok(ok_response(RemoveResponse {
        deleted: true,
        reason,
    }))
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    deleted: bool,
    reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_trim_and_blank_is_none() {
        assert_eq!(
            optional_reason(Some("  policy  ".to_owned())),
            Some("policy".to_owned())
        );
        assert_eq!(optional_reason(Some("  ".to_owned())), None);
    }
}
