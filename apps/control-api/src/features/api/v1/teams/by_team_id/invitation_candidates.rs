use axum::{extract::Query, response::IntoResponse};
use serde::Deserialize;

use crate::{
    domain::{registration::SignupPolicy, settings, teams},
    infra::{
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/teams/{team_id}/invitation-candidates",
        axum::routing::get(candidates),
    )
}

#[derive(Deserialize)]
struct InvitationCandidatesQuery {
    q: String,
}

async fn candidates(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
    Query(query): Query<InvitationCandidatesQuery>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_admin("teams.invitations.candidates.admin_required")?;
    let db = crate::infra::http::database(&state, "teams.invitations.candidates.no_database")?;
    let policy = settings::get_setting(db, "signup.policy")
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.invitations.candidates.read_policy",
            source,
        })?
        .and_then(|setting| setting.value.as_str().map(str::to_owned));
    let policy = SignupPolicy::parse(policy.as_deref()).map_err(|error| AppError::Internal {
        op: "teams.invitations.candidates.invalid_policy",
        message: error.to_string(),
    })?;
    let candidates = teams::invitation_candidates(db, policy, &query.q, 10)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.invitations.candidates.search",
            source,
        })?;

    Ok(ok_response(CandidatesResponse {
        candidates: candidates
            .into_iter()
            .map(|candidate| CandidatesCandidatesResponse {
                kind: if candidate.user_id.is_some() {
                    "user"
                } else {
                    "email"
                },
                user_id: candidate.user_id,
                email: candidate.email,
                display_name: candidate.display_name,
            })
            .collect::<Vec<_>>(),
    }))
}

#[derive(serde::Serialize)]
struct CandidatesCandidatesResponse {
    kind: &'static str,
    user_id: Option<uuid::Uuid>,
    email: String,
    display_name: Option<String>,
}

#[derive(serde::Serialize)]
struct CandidatesResponse {
    candidates: Vec<CandidatesCandidatesResponse>,
}
