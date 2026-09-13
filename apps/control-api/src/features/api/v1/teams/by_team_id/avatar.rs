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
        database::entity::team,
        error::{AppError, ok_response},
        http::extractors::TeamRole,
        storage::{StorageManager, StorageWriteGuard},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/teams/{team_id}/avatar",
            axum::routing::delete(delete_team).put(upload_team),
        )
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024))
}

#[derive(Debug, thiserror::Error)]
enum AvatarError {
    #[error("avatar must be a valid PNG image")]
    InvalidImage,
    #[error("avatar must be square and between 128 and 1024 pixels")]
    InvalidGeometry,
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

fn team_avatar_key(team_id: Uuid, version: Uuid) -> String {
    format!("avatars/teams/{team_id}/{version}.webp")
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

    let update = replace_team_version(&state, team_role.team_id, Some(version), OP).await;
    let (team, old_version) = match update {
        Ok(result) => result,
        Err(error) => {
            let _ = write_guard.remove(&key).await;
            return Err(error);
        }
    };
    remove_old(
        &write_guard,
        old_version.map(|old| team_avatar_key(team.id, old)),
        OP,
    )
    .await;
    Ok(ok_response(UploadTeamResponse {
        team: team_data(&team),
    }))
}

async fn delete_team(
    State(state): State<ControlApiState>,
    team_role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "avatars.team.delete";
    team_role.require_owner("avatars.team.owner_required")?;
    let write_guard =
        storage(&state)
            .begin_write()
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
    let (team, old_version) = replace_team_version(&state, team_role.team_id, None, OP).await?;
    remove_old(
        &write_guard,
        old_version.map(|old| team_avatar_key(team.id, old)),
        OP,
    )
    .await;
    Ok(ok_response(DeleteTeamResponse {
        team: team_data(&team),
    }))
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

fn team_data(team: &team::Model) -> TeamResponse {
    TeamResponse {
        id: team.id,
        slug: team.slug.clone(),
        name: team.name.clone(),
        kind: kind_value(&team.kind),
        owner_user_id: team.owner_user_id,
        group_id: team.group_id,
        avatar_url: team_avatar_url(team.id, team.avatar_version),
    }
}

async fn remove_old(storage: &StorageWriteGuard, key: Option<String>, op: &'static str) {
    if let Some(key) = key
        && let Err(error) = storage.remove(&key).await
    {
        tracing::warn!(operation = op, %error, storage_key = %key, "failed to remove replaced avatar");
    }
}

pub(crate) fn kind_value(kind: &crate::infra::database::entity::TeamKind) -> &'static str {
    use crate::infra::database::entity::TeamKind;

    match kind {
        TeamKind::Personal => "personal",
        TeamKind::Team => "team",
    }
}

#[derive(serde::Serialize)]
struct TeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
    kind: &'static str,
    owner_user_id: Option<uuid::Uuid>,
    group_id: Option<uuid::Uuid>,
    avatar_url: Option<String>,
}

#[derive(serde::Serialize)]
struct UploadTeamResponse {
    team: TeamResponse,
}

#[derive(serde::Serialize)]
struct DeleteTeamResponse {
    team: TeamResponse,
}
