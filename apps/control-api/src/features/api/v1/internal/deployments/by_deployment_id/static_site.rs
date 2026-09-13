use axum::{
    Extension,
    body::Body,
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
};
use grass_node_protocol::artifact_headers;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        deployments,
        quotas::QuotaDimension,
        scheduler::{self, NodeUsage},
        teams,
    },
    infra::{
        audit::{self as audits, CreateAuditEventParams},
        database::entity::{
            AuditEventResult, DeploymentArtifactKind, deployment, deployment_artifact, node, team,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
        quota::{QuotaCharge, QuotaService},
        storage::StorageError,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UploadArtifactResponse {
    pub artifact_id: Uuid,
    pub size_bytes: i64,
    pub checksum_sha256: String,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/deployments/{deployment_id}/static-site",
        axum::routing::put(upload_static_site),
    )
}

async fn team_for(
    db: &sea_orm::DatabaseConnection,
    team_id: Uuid,
    op: &'static str,
) -> Result<team::Model, AppError> {
    teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "team not found".to_owned(),
        })
}

async fn build_owned_deployment(
    db: &sea_orm::DatabaseConnection,
    node: &node::Model,
    deployment_id: Uuid,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        })?;
    if deployment.build_node_id != Some(node.id) {
        return Err(AppError::Forbidden {
            op,
            message: "deployment build is not assigned to this node".to_owned(),
        });
    }
    Ok(deployment)
}

// --- Artifact upload / download ---------------------------------------------
const BYTES_PER_MB: u64 = 1024 * 1024;

#[derive(Debug)]
struct ArtifactUploadMetadata {
    checksum_sha256: String,
    packed_size_bytes: u64,
    unpacked_size_bytes: u64,
    disk_mb: i64,
}

fn parse_upload_metadata(headers: &HeaderMap) -> Result<ArtifactUploadMetadata, String> {
    let required = |name: &'static str| {
        headers
            .get(name)
            .ok_or_else(|| format!("missing {name} header"))?
            .to_str()
            .map(str::to_owned)
            .map_err(|_| format!("invalid {name} header"))
    };
    let checksum_sha256 = required(artifact_headers::CHECKSUM_SHA256)?;
    if checksum_sha256.len() != 64
        || !checksum_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "invalid {} header",
            artifact_headers::CHECKSUM_SHA256
        ));
    }
    let parse_size = |name: &'static str| -> Result<u64, String> {
        let value = required(name)?;
        let value = value
            .parse::<u64>()
            .map_err(|_| format!("invalid {name} header"))?;
        if value == 0 {
            return Err(format!("invalid {name} header"));
        }
        Ok(value)
    };
    let packed_size_bytes = parse_size(artifact_headers::PACKED_SIZE_BYTES)?;
    let unpacked_size_bytes = parse_size(artifact_headers::UNPACKED_SIZE_BYTES)?;
    let disk_mb = i64::try_from(unpacked_size_bytes.div_ceil(BYTES_PER_MB))
        .map_err(|_| "unpacked artifact size exceeds the supported range".to_owned())?;
    Ok(ArtifactUploadMetadata {
        checksum_sha256,
        packed_size_bytes,
        unpacked_size_bytes,
        disk_mb,
    })
}

fn validate_serve_disk(
    capacity_disk_mb: i64,
    current_deployment_disk_mb: i64,
    usage: NodeUsage,
    actual_disk_mb: i64,
) -> Result<(), String> {
    let current = u64::try_from(current_deployment_disk_mb)
        .map_err(|_| "current deployment disk usage is invalid".to_owned())?;
    let actual =
        u64::try_from(actual_disk_mb).map_err(|_| "artifact disk usage is invalid".to_owned())?;
    let capacity = u64::try_from(capacity_disk_mb)
        .map_err(|_| "assigned Serve Node disk capacity is invalid".to_owned())?;
    let required = usage.disk_mb.saturating_sub(current).saturating_add(actual);
    if required > capacity {
        return Err(format!(
            "artifact needs {required} MB but the assigned Serve Node has {capacity} MB"
        ));
    }
    Ok(())
}

fn optional_header(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// PUT /api/v1/internal/deployments/{deployment_id}/static-site
pub async fn upload_static_site(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.static_site";
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;
    let deployment = build_owned_deployment(db, &node, deployment_id, OP).await?;
    let upload = parse_upload_metadata(&headers)
        .map_err(|message| AppError::Validation { op: OP, message })?;
    let team = team_for(db, deployment.team_id, OP).await?;
    let quota = QuotaService::new(db, cache);
    let artifact_max_mb = quota
        .scalar_limit(OP, &team, QuotaDimension::ArtifactMaxMb)
        .await?;
    let max_bytes = match artifact_max_mb {
        Some(max_mb) => u64::try_from(max_mb)
            .ok()
            .and_then(|max_mb| max_mb.checked_mul(BYTES_PER_MB))
            .ok_or_else(|| AppError::Validation {
                op: OP,
                message: "artifact quota is invalid".to_owned(),
            })?,
        None => u64::MAX,
    };
    if upload.packed_size_bytes > max_bytes {
        return Err(crate::infra::quota::quota_exceeded_error(
            OP,
            QuotaDimension::ArtifactMaxMb,
        ));
    }
    let pending = match state
        .storage
        .clone()
        .write_artifact_stream(
            deployment.project_id,
            deployment.id,
            body.into_data_stream(),
            max_bytes,
        )
        .await
    {
        Ok(pending) => pending,
        Err(StorageError::LimitExceeded { .. }) => {
            return Err(crate::infra::quota::quota_exceeded_error(
                OP,
                QuotaDimension::ArtifactMaxMb,
            ));
        }
        Err(source) => {
            return Err(AppError::Infrastructure {
                op: OP,
                source: source.into(),
            });
        }
    };
    if pending.size_bytes == 0 {
        pending.discard().await;
        return Err(AppError::Validation {
            op: OP,
            message: "artifact body is empty".to_owned(),
        });
    }
    if u64::try_from(pending.size_bytes).ok() != Some(upload.packed_size_bytes) {
        pending.discard().await;
        return Err(AppError::Validation {
            op: OP,
            message: "artifact packed size does not match its metadata".to_owned(),
        });
    }
    if pending.checksum_sha256 != upload.checksum_sha256 {
        pending.discard().await;
        return Err(AppError::Validation {
            op: OP,
            message: "artifact checksum does not match its metadata".to_owned(),
        });
    }
    let size_mb = i64::try_from(upload.packed_size_bytes.div_ceil(BYTES_PER_MB)).map_err(|_| {
        AppError::Validation {
            op: OP,
            message: "artifact packed size exceeds the supported range".to_owned(),
        }
    })?;
    let reservation = match quota
        .reserve(
            OP,
            &team,
            None,
            &[QuotaCharge::amount(QuotaDimension::StorageMb, size_mb)],
        )
        .await
    {
        Ok(reservation) => reservation,
        Err(error) => {
            pending.discard().await;
            return Err(error);
        }
    };
    let manifest = json!({
        "runtime_kind": optional_header(&headers, artifact_headers::RUNTIME_KIND),
        "output_api_version": optional_header(&headers, artifact_headers::OUTPUT_API_VERSION),
        "framework_name": optional_header(&headers, artifact_headers::FRAMEWORK_NAME),
        "framework_version": optional_header(&headers, artifact_headers::FRAMEWORK_VERSION),
        "packed_size_bytes": upload.packed_size_bytes,
        "unpacked_size_bytes": upload.unpacked_size_bytes,
    });
    let team_id = deployment.team_id;
    let project_id = deployment.project_id;
    let result: Result<_, AppError> = async move {
        let transaction = crate::infra::audit::AuditTransaction::begin(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        scheduler::lock_placement(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        let current = deployments::get_by_id(&transaction, deployment_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .ok_or_else(|| AppError::NotFound {
                op: OP,
                message: "deployment not found".to_owned(),
            })?;
        if current.build_node_id != Some(node.id) {
            return Err(AppError::Forbidden {
                op: OP,
                message: "deployment build is not assigned to this node".to_owned(),
            });
        }
        let serve_node_id = current.serve_node_id.ok_or_else(|| AppError::Validation {
            op: OP,
            message: "deployment is not assigned to a Serve Node".to_owned(),
        })?;
        let serve_node = node::Entity::find_by_id(serve_node_id)
            .filter(node::Column::DeletedAt.is_null())
            .one(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .ok_or_else(|| AppError::Validation {
                op: OP,
                message: "assigned Serve Node does not exist".to_owned(),
            })?;
        if !serve_node.serve_enabled {
            return Err(AppError::Validation {
                op: OP,
                message: "assigned node does not have Serve capability".to_owned(),
            });
        }
        let usage = scheduler::node_usage(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .remove(&serve_node_id)
            .unwrap_or_default();
        validate_serve_disk(
            serve_node.capacity_disk_mb,
            current.serve_disk_mb,
            usage,
            upload.disk_mb,
        )
        .map_err(|message| AppError::Validation { op: OP, message })?;

        let mut active: deployment::ActiveModel = current.into();
        active.serve_disk_mb = Set(upload.disk_mb);
        active
            .update(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;

        let stored = pending
            .finalize()
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        let existing = deployment_artifact::Entity::find()
            .filter(deployment_artifact::Column::DeploymentId.eq(deployment_id))
            .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
            .one(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        let artifact = if let Some(existing) = existing {
            let mut active: deployment_artifact::ActiveModel = existing.into();
            active.storage_path = Set(stored.relative_path.clone());
            active.checksum_sha256 = Set(Some(stored.checksum_sha256.clone()));
            active.size_bytes = Set(Some(stored.size_bytes));
            active.manifest = Set(manifest);
            active.deleted_at = Set(None);
            active.update(&transaction).await
        } else {
            deployment_artifact::ActiveModel {
                id: Set(Uuid::now_v7()),
                deployment_id: Set(deployment_id),
                kind: Set(DeploymentArtifactKind::GrassOutput),
                storage_path: Set(stored.relative_path.clone()),
                checksum_sha256: Set(Some(stored.checksum_sha256.clone())),
                size_bytes: Set(Some(stored.size_bytes)),
                manifest: Set(manifest),
                deleted_at: Set(None),
                created_at: Set(time::OffsetDateTime::now_utc()),
            }
            .insert(&transaction)
            .await
        }
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
        audits::create_audit_event(
            &transaction,
            CreateAuditEventParams {
                actor_user_id: None,
                actor_node_id: Some(node.id),
                team_id: Some(team_id),
                action: "artifact.uploaded".to_owned(),
                target_type: "deployment".to_owned(),
                target_id: Some(deployment_id),
                result: AuditEventResult::Success,
                reason: None,
                metadata: json!({
                    "size_bytes": stored.size_bytes,
                    "unpacked_size_bytes": upload.unpacked_size_bytes,
                    "checksum_sha256": stored.checksum_sha256,
                    "project_id": project_id,
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
        Ok((artifact, stored))
    }
    .await;
    let (artifact, stored) = match result {
        Ok(result) => result,
        Err(error) => {
            quota.rollback(reservation).await;
            return Err(error);
        }
    };

    quota
        .commit(OP, reservation, "deployment_artifact", Some(artifact.id))
        .await?;

    Ok(ok_response(UploadArtifactResponse {
        artifact_id: artifact.id,
        size_bytes: stored.size_bytes,
        checksum_sha256: stored.checksum_sha256,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_metadata_requires_sizes_and_checksum() {
        let mut headers = HeaderMap::new();
        headers.insert(
            artifact_headers::CHECKSUM_SHA256,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .parse()
                .unwrap(),
        );
        headers.insert(artifact_headers::PACKED_SIZE_BYTES, "9".parse().unwrap());
        headers.insert(
            artifact_headers::UNPACKED_SIZE_BYTES,
            "1025".parse().unwrap(),
        );

        let metadata = parse_upload_metadata(&headers).unwrap();

        assert_eq!(metadata.packed_size_bytes, 9);
        assert_eq!(metadata.unpacked_size_bytes, 1025);
        assert_eq!(metadata.disk_mb, 1);

        headers.remove(artifact_headers::UNPACKED_SIZE_BYTES);
        assert_eq!(
            parse_upload_metadata(&headers).unwrap_err(),
            "missing x-grass-unpacked-size-bytes header"
        );
    }

    #[test]
    fn actual_disk_replaces_the_deployments_reserved_disk() {
        let usage = NodeUsage {
            cpu_millicores: 400,
            memory_mb: 512,
            disk_mb: 900,
            deployments: 3,
        };

        assert!(validate_serve_disk(1_024, 512, usage, 600).is_ok());
        assert_eq!(
            validate_serve_disk(1_024, 512, usage, 700).unwrap_err(),
            "artifact needs 1088 MB but the assigned Serve Node has 1024 MB"
        );
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            UploadArtifactResponse,
            grass_node_protocol::UploadArtifactResponse,
        >("UploadArtifactResponse");
    }
}
