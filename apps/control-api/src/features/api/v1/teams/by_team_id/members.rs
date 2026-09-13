pub(crate) mod by_user_id;

use axum::response::IntoResponse;

use crate::{
    domain::teams,
    infra::{
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/teams/{team_id}/members", axum::routing::get(list))
        .merge(by_user_id::router())
}

fn role_value(role: &crate::infra::database::entity::TeamMemberRole) -> &'static str {
    use crate::infra::database::entity::TeamMemberRole;

    match role {
        TeamMemberRole::Owner => "owner",
        TeamMemberRole::Admin => "admin",
        TeamMemberRole::Member => "member",
        TeamMemberRole::Viewer => "viewer",
    }
}

async fn list(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    let db = crate::infra::http::database(&state, "teams.members.list.no_database")?;
    let members = teams::list_members(db, team_role.team_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.members.list",
            source,
        })?;

    Ok(ok_response(ListResponse {
        members: members
            .into_iter()
            .map(|(member, user)| ListMembersResponse {
                id: member.id,
                user_id: user.id,
                email: user.email.clone(),
                display_name: user.display_name.clone(),
                role: role_value(&member.role),
                joined_at: member.joined_at,
            })
            .collect::<Vec<_>>(),
    }))
}

#[derive(serde::Serialize)]
struct ListMembersResponse {
    id: uuid::Uuid,
    user_id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    role: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    joined_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ListResponse {
    members: Vec<ListMembersResponse>,
}
