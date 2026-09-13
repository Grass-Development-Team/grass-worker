use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, header},
    response::IntoResponse,
};
use image::{ImageFormat, ImageReader, Limits};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QuerySelect,
    TransactionTrait,
};
use std::io::Cursor;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    infra::{
        database::entity::user,
        error::{AppError, ok_response},
        http::extractors::Session,
        storage::{StorageManager, StorageWriteGuard},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/me/avatar",
            axum::routing::delete(delete_user).put(upload_user),
        )
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024))
}

fn user_data(user: &user::Model) -> UserResponse {
    UserResponse {
        id: user.id,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        avatar_url: user_avatar_url(user.id, user.avatar_version),
        platform_role: user.platform_role.as_str(),
        email_verified: user.email_verified_at.is_some(),
    }
}

#[derive(Debug, thiserror::Error)]
enum AvatarError {
    #[error("avatar must be a valid PNG image")]
    InvalidImage,
    #[error("avatar must be square and between 128 and 1024 pixels")]
    InvalidGeometry,
}

fn user_avatar_url(user_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/users/{user_id}/{version}/avatar.webp"))
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

fn storage(state: &ControlApiState) -> StorageManager {
    state.storage.clone()
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
    let write_guard = storage
        .begin_write()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    write_guard
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
            let _ = write_guard.remove(&key).await;
            return Err(error);
        }
    };
    remove_old(
        &write_guard,
        old_version.map(|old| user_avatar_key(user.id, old)),
        OP,
    )
    .await;
    Ok(ok_response(UploadUserResponse {
        user: user_data(&user),
    }))
}

async fn delete_user(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "avatars.user.delete";
    let write_guard =
        storage(&state)
            .begin_write()
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
    let (user, old_version) = replace_user_version(&state, session.data.user_id, None, OP).await?;
    remove_old(
        &write_guard,
        old_version.map(|old| user_avatar_key(user.id, old)),
        OP,
    )
    .await;
    Ok(ok_response(DeleteUserResponse {
        user: user_data(&user),
    }))
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

async fn remove_old(storage: &StorageWriteGuard, key: Option<String>, op: &'static str) {
    if let Some(key) = key
        && let Err(error) = storage.remove(&key).await
    {
        tracing::warn!(operation = op, %error, storage_key = %key, "failed to remove replaced avatar");
    }
}

#[derive(serde::Serialize)]
struct UserResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    platform_role: &'static str,
    email_verified: bool,
}

#[derive(serde::Serialize)]
struct UploadUserResponse {
    user: UserResponse,
}

#[derive(serde::Serialize)]
struct DeleteUserResponse {
    user: UserResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ExtendedColorType;
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;

    use axum::http::HeaderMap;
    use axum::http::header;
    use uuid::Uuid;
    pub(crate) fn team_avatar_url(team_id: Uuid, version: Option<Uuid>) -> Option<String> {
        version.map(|version| format!("/api/v1/avatars/teams/{team_id}/{version}/avatar.webp"))
    }

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
