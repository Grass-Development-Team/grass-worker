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
        database::entity::user, error::AppError, http::extractors::Session, storage::StorageManager,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/avatars/users/{user_id}/{version}/avatar.webp",
            axum::routing::get(read_user),
        )
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024))
}

fn user_avatar_key(user_id: Uuid, version: Uuid) -> String {
    format!("avatars/users/{user_id}/{version}.webp")
}

fn storage(state: &ControlApiState) -> StorageManager {
    state.storage.clone()
}

async fn read_user(
    State(state): State<ControlApiState>,
    _session: Session,
    Path((user_id, version)): Path<(Uuid, Uuid)>,
) -> Result<Response, AppError> {
    const OP: &str = "avatars.user.read";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let exists = user::Entity::find_by_id(user_id)
        .filter(user::Column::AvatarVersion.eq(version))
        .filter(user::Column::DeletedAt.is_null())
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
    read_object(&storage(&state), &user_avatar_key(user_id, version), OP).await
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
