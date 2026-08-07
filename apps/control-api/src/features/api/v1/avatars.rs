use std::io::Cursor;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use image::{ImageFormat, ImageReader, Limits};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QuerySelect,
    TransactionTrait,
};
use time::OffsetDateTime;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    infra::{
        database::entity::{team, user},
        error::{AppError, ok_response},
        http::extractors::{Session, TeamRole},
        storage::LocalStorage,
    },
    state::ControlApiState,
};

const MAX_UPLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
enum AvatarError {
    #[error("avatar must be a valid PNG image")]
    InvalidImage,
    #[error("avatar must be square and between 128 and 1024 pixels")]
    InvalidGeometry,
}

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/me/avatar", put(upload_user).delete(delete_user))
        .route(
            "/teams/{team_id}/avatar",
            put(upload_team).delete(delete_team),
        )
        .route(
            "/avatars/users/{user_id}/{version}/avatar.webp",
            get(read_user),
        )
        .route(
            "/avatars/teams/{team_id}/{version}/avatar.webp",
            get(read_team),
        )
        .layer(axum::extract::DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
}

pub(crate) fn user_avatar_url(user_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/users/{user_id}/{version}/avatar.webp"))
}

pub(crate) fn team_avatar_url(team_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/teams/{team_id}/{version}/avatar.webp"))
}

fn encode_avatar(png: &[u8]) -> Result<Vec<u8>, AvatarError> {
    let mut reader = ImageReader::with_format(Cursor::new(png), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(8 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| AvatarError::InvalidImage)?;
    let (width, height) = (image.width(), image.height());
    if width != height || !(128..=1024).contains(&width) {
        return Err(AvatarError::InvalidGeometry);
    }
    let rgba = image.into_rgba8();
    Ok(webp::Encoder::from_rgba(&rgba, width, height)
        .encode(85.0)
        .to_vec())
}

fn user_avatar_key(user_id: Uuid, version: Uuid) -> String {
    format!("avatars/users/{user_id}/{version}.webp")
}

fn team_avatar_key(team_id: Uuid, version: Uuid) -> String {
    format!("avatars/teams/{team_id}/{version}.webp")
}

fn storage(state: &ControlApiState) -> LocalStorage {
    LocalStorage::new(state.config.read().unwrap().storage.root.clone())
}

fn require_png(headers: &HeaderMap, op: &'static str) -> Result<(), AppError> {
    let is_png = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("image/png"));
    if !is_png {
        return Err(AppError::Validation {
            op,
            message: "avatar content type must be image/png".to_owned(),
        });
    }
    Ok(())
}

async fn upload_user(
    State(state): State<ControlApiState>,
    session: Session,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "avatars.user.upload";
    require_png(&headers, OP)?;
    let webp = encode_avatar(&body).map_err(|error| AppError::Validation {
        op: OP,
        message: error.to_string(),
    })?;
    let version = Uuid::now_v7();
    let key = user_avatar_key(session.data.user_id, version);
    let storage = storage(&state);
    storage
        .write_bytes(&key, &webp)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let update = replace_user_version(&state, session.data.user_id, Some(version), OP).await;
    let (user, old_version) = match update {
        Ok(result) => result,
        Err(error) => {
            let _ = storage.remove(&key).await;
            return Err(error);
        }
    };
    remove_old(
        &storage,
        old_version.map(|old| user_avatar_key(user.id, old)),
        OP,
    )
    .await;
    Ok(ok_response(serde_json::json!({
        "user": super::auth::user_data(&user),
    })))
}

async fn delete_user(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "avatars.user.delete";
    let (user, old_version) = replace_user_version(&state, session.data.user_id, None, OP).await?;
    remove_old(
        &storage(&state),
        old_version.map(|old| user_avatar_key(user.id, old)),
        OP,
    )
    .await;
    Ok(ok_response(serde_json::json!({
        "user": super::auth::user_data(&user),
    })))
}

async fn replace_user_version(
    state: &ControlApiState,
    user_id: Uuid,
    version: Option<Uuid>,
    op: &'static str,
) -> Result<(user::Model, Option<Uuid>), AppError> {
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op,
        message: "database not available".to_owned(),
    })?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let current = user::Entity::find_by_id(user_id)
        .filter(user::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "user not found".to_owned(),
        })?;
    let old_version = current.avatar_version;
    let mut active: user::ActiveModel = current.into();
    active.avatar_version = Set(version);
    active.updated_at = Set(OffsetDateTime::now_utc());
    let updated = active
        .update(&transaction)
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
    Ok((updated, old_version))
}

async fn upload_team(
    State(state): State<ControlApiState>,
    team_role: TeamRole,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "avatars.team.upload";
    team_role.require_owner("avatars.team.owner_required")?;
    require_png(&headers, OP)?;
    let webp = encode_avatar(&body).map_err(|error| AppError::Validation {
        op: OP,
        message: error.to_string(),
    })?;
    let version = Uuid::now_v7();
    let key = team_avatar_key(team_role.team_id, version);
    let storage = storage(&state);
    storage
        .write_bytes(&key, &webp)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let update = replace_team_version(&state, team_role.team_id, Some(version), OP).await;
    let (team, old_version) = match update {
        Ok(result) => result,
        Err(error) => {
            let _ = storage.remove(&key).await;
            return Err(error);
        }
    };
    remove_old(
        &storage,
        old_version.map(|old| team_avatar_key(team.id, old)),
        OP,
    )
    .await;
    Ok(ok_response(serde_json::json!({
        "team": team_data(&team),
    })))
}

async fn delete_team(
    State(state): State<ControlApiState>,
    team_role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "avatars.team.delete";
    team_role.require_owner("avatars.team.owner_required")?;
    let (team, old_version) = replace_team_version(&state, team_role.team_id, None, OP).await?;
    remove_old(
        &storage(&state),
        old_version.map(|old| team_avatar_key(team.id, old)),
        OP,
    )
    .await;
    Ok(ok_response(serde_json::json!({
        "team": team_data(&team),
    })))
}

async fn replace_team_version(
    state: &ControlApiState,
    team_id: Uuid,
    version: Option<Uuid>,
    op: &'static str,
) -> Result<(team::Model, Option<Uuid>), AppError> {
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op,
        message: "database not available".to_owned(),
    })?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let current = team::Entity::find_by_id(team_id)
        .filter(team::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "team not found".to_owned(),
        })?;
    let old_version = current.avatar_version;
    let mut active: team::ActiveModel = current.into();
    active.avatar_version = Set(version);
    active.updated_at = Set(OffsetDateTime::now_utc());
    let updated = active
        .update(&transaction)
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
    Ok((updated, old_version))
}

fn team_data(team: &team::Model) -> serde_json::Value {
    serde_json::json!({
        "id": team.id,
        "slug": team.slug,
        "name": team.name,
        "kind": super::teams::kind_value(&team.kind),
        "owner_user_id": team.owner_user_id,
        "group_id": team.group_id,
        "avatar_url": team_avatar_url(team.id, team.avatar_version),
    })
}

async fn remove_old(storage: &LocalStorage, key: Option<String>, op: &'static str) {
    if let Some(key) = key
        && let Err(error) = storage.remove(&key).await
    {
        tracing::warn!(operation = op, %error, storage_key = %key, "failed to remove replaced avatar");
    }
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
    storage: &LocalStorage,
    key: &str,
    op: &'static str,
) -> Result<Response, AppError> {
    let object = storage
        .open_artifact(key)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
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
        .body(Body::from_stream(ReaderStream::new(object.file)))
        .map_err(|error| AppError::Internal {
            op,
            message: format!("failed to build avatar response: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let pixels = vec![127_u8; width as usize * height as usize * 4];
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded)
            .write_image(&pixels, width, height, ExtendedColorType::Rgba8)
            .unwrap();
        encoded
    }

    #[test]
    fn valid_square_png_is_reencoded_as_webp() {
        let encoded = encode_avatar(&png(128, 128)).unwrap();

        assert!(encoded.starts_with(b"RIFF"));
        assert_eq!(&encoded[8..12], b"WEBP");
    }

    #[test]
    fn invalid_avatar_geometry_is_rejected() {
        assert!(encode_avatar(&png(128, 129)).is_err());
        assert!(encode_avatar(&png(127, 127)).is_err());
        assert!(encode_avatar(&png(1025, 1025)).is_err());
        assert!(encode_avatar(b"not a png").is_err());
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

    #[test]
    fn avatar_urls_are_immutable_and_absent_without_a_version() {
        let owner_id = Uuid::nil();
        let version = Uuid::max();

        assert_eq!(
            user_avatar_url(owner_id, Some(version)),
            Some(format!(
                "/api/v1/avatars/users/{owner_id}/{version}/avatar.webp"
            ))
        );
        assert_eq!(team_avatar_url(owner_id, None), None);
    }

    #[test]
    fn upload_content_type_must_be_png() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            "image/png; charset=binary".parse().unwrap(),
        );
        assert!(require_png(&headers, "test.avatar").is_ok());

        headers.insert(header::CONTENT_TYPE, "image/jpeg".parse().unwrap());
        assert!(require_png(&headers, "test.avatar").is_err());
        assert!(require_png(&HeaderMap::new(), "test.avatar").is_err());
    }
}
