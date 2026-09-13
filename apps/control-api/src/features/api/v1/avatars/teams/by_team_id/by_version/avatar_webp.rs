use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::{
    infra::{
        database::entity::team, error::AppError, http::extractors::Session, storage::StorageManager,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/avatars/teams/{team_id}/{version}/avatar.webp",
            axum::routing::get(read_team),
        )
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024))
}

fn team_avatar_key(team_id: Uuid, version: Uuid) -> String {
    format!("avatars/teams/{team_id}/{version}.webp")
}

fn storage(state: &ControlApiState) -> StorageManager {
    state.storage.clone()
}

async fn read_team(
    State(state): State<ControlApiState>,
    _session: Session,
    Path((team_id, version)): Path<(Uuid, Uuid)>,
) -> Result<Response, AppError> {
    const OP: &str = "avatars.team.read";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let exists = team::Entity::find_by_id(team_id)
        .filter(team::Column::AvatarVersion.eq(version))
        .filter(team::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .is_some();
    if !exists {
        return Err(AppError::NotFound {
            op: OP,
            message: "avatar not found".to_owned(),
        });
    }
    read_object(&storage(&state), &team_avatar_key(team_id, version), OP).await
}

async fn read_object(
    storage: &StorageManager,
    key: &str,
    op: &'static str,
) -> Result<Response, AppError> {
    let object = storage
        .open_artifact(key)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "avatar not found".to_owned(),
        })?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/webp")
        .header(header::CONTENT_LENGTH, object.size_bytes)
        .header(
            header::CACHE_CONTROL,
            "private, max-age=31536000, immutable",
        )
        .body(Body::from_stream(object.stream))
        .map_err(|error| AppError::Internal {
            op,
            message: format!("failed to build avatar response: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    fn user_avatar_key(user_id: Uuid, version: Uuid) -> String {
        format!("avatars/users/{user_id}/{version}.webp")
    }

    #[test]
    fn avatar_keys_are_backend_neutral_and_versioned() {
        let owner_id = Uuid::parse_str("018f47e2-3d62-7cc3-b0fd-8a73f01b2a10").unwrap();
        let version = Uuid::parse_str("018f47e2-3d62-7cc3-b0fd-8a73f01b2a11").unwrap();

        assert_eq!(
            user_avatar_key(owner_id, version),
            "avatars/users/018f47e2-3d62-7cc3-b0fd-8a73f01b2a10/018f47e2-3d62-7cc3-b0fd-8a73f01b2a11.webp"
        );
        assert_eq!(
            team_avatar_key(owner_id, version),
            "avatars/teams/018f47e2-3d62-7cc3-b0fd-8a73f01b2a10/018f47e2-3d62-7cc3-b0fd-8a73f01b2a11.webp"
        );
    }
}
