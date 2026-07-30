use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        hosts::{self, AutoAssignSelection},
        projects::{self, CreateProjectParams},
        quotas::QuotaDimension,
        teams,
    },
    infra::{
        database::entity::{
            AuditEventResult, HostBindingEnvironment, HostBindingKind, HostReviewStatus,
            ProjectRuntime,
        },
        error::{AppError, ok_response},
        host_provision::service::{BindHostRequest, HostBindingService},
        http::extractors::Session,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub team_id: Uuid,
    pub name: String,
    pub slug: String,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default)]
    pub repository_url: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub root_directory: Option<String>,
    #[serde(default)]
    pub install_command: Option<String>,
    #[serde(default)]
    pub build_command: Option<String>,
    #[serde(default)]
    pub output_directory: Option<String>,
    #[serde(default)]
    pub framework_hint: Option<String>,
}

fn default_runtime() -> String {
    "static".to_owned()
}

/// POST /api/v1/projects
pub async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<CreateProjectRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.create";

    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    let team = teams::get_by_id(db, body.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team not found".to_owned(),
        })?;
    let role = teams::member_role(db, team.id, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Forbidden {
            op: OP,
            message: "not a member of this team".to_owned(),
        })?;
    if matches!(role, crate::infra::database::entity::TeamMemberRole::Viewer) {
        return Err(AppError::Forbidden {
            op: OP,
            message: "member role required".to_owned(),
        });
    }

    if body.name.trim().is_empty() {
        return Err(AppError::Validation {
            op: OP,
            message: "name is required".to_owned(),
        });
    }
    let slug =
        grass_validator::normalize_slug(&body.slug).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    let runtime =
        projects::parse_creatable_runtime(&body.runtime).ok_or_else(|| AppError::Validation {
            op: OP,
            message: format!(
                "runtime {} is not supported; first-stage projects are static or ssr",
                body.runtime
            ),
        })?;
    if let Some(url) = &body.repository_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            super::validate_repository_url(trimmed).map_err(|message| AppError::Validation {
                op: OP,
                message: message.to_owned(),
            })?;
        }
    }

    // Project quota: total plus the runtime-specific dimension.
    let runtime_dimension = match runtime {
        ProjectRuntime::Ssr => QuotaDimension::ProjectsSsr,
        _ => QuotaDimension::ProjectsStatic,
    };
    let quota = QuotaService::new(db, cache);
    let reservation = quota
        .reserve(
            OP,
            &team,
            Some(session.data.user_id),
            &[
                QuotaCharge::one(QuotaDimension::Projects),
                QuotaCharge::one(runtime_dimension),
            ],
        )
        .await?;

    let source_config = json!({
        "root_directory": super::optional_trimmed(body.root_directory),
        "framework_hint": super::optional_trimmed(body.framework_hint),
    });

    let project = match projects::create_project(
        db,
        CreateProjectParams {
            team_id: team.id,
            slug,
            name: body.name.trim().to_owned(),
            runtime,
            repository_url: super::optional_trimmed(body.repository_url),
            default_branch: super::optional_trimmed(body.default_branch),
            install_command: super::optional_trimmed(body.install_command),
            build_command: super::optional_trimmed(body.build_command),
            output_directory: super::optional_trimmed(body.output_directory),
            source_config,
            build_config: json!({}),
        },
    )
    .await
    {
        Ok(project) => project,
        Err(source) => {
            quota.rollback(reservation).await;
            return Err(if crate::infra::database::is_unique_violation(&source) {
                AppError::Conflict {
                    op: OP,
                    message: "project slug is already in use in this team".to_owned(),
                }
            } else {
                AppError::Infrastructure { op: OP, source }
            });
        }
    };

    quota
        .commit(OP, reservation, "project", Some(project.id))
        .await?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: Some(team.id),
            action: "project.created".to_owned(),
            target_type: "project".to_owned(),
            target_id: Some(project.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "team_id": team.id, "slug": project.slug }),
        },
    )
    .await;

    // Platform-domain auto-assignment. Failures never fail project creation;
    // the response carries the reason so the Console can explain it.
    let host_assignment = auto_assign_host(&state, &session, &team, &project).await;

    Ok(ok_response(json!({
        "project": super::project_view(&project),
        "host_assignment": host_assignment,
    })))
}

async fn auto_assign_host(
    state: &ControlApiState,
    session: &Session,
    team: &crate::infra::database::entity::team::Model,
    project: &crate::infra::database::entity::project::Model,
) -> serde_json::Value {
    const OP: &str = "projects.create.auto_assign_host";

    let (Ok(db), Ok(cache)) = (super::database(state, OP), super::cache(state, OP)) else {
        return json!({ "assigned": false, "reason": "infrastructure unavailable" });
    };

    let policy = match hosts::policy_for_team_group(db, team.group_id).await {
        Ok(policy) => policy,
        Err(error) => {
            tracing::warn!(operation = OP, %error, "failed to load host policy");
            return json!({ "assigned": false, "reason": "host policy unavailable" });
        }
    };
    if let Some(policy) = &policy
        && !policy.allow_auto_assign
    {
        return json!({
            "assigned": false,
            "reason": "team group does not allow automatic host assignment",
        });
    }

    let sources = match hosts::list_sources(db).await {
        Ok(sources) => sources,
        Err(error) => {
            tracing::warn!(operation = OP, %error, "failed to list host sources");
            return json!({ "assigned": false, "reason": "host sources unavailable" });
        }
    };

    let source = match hosts::select_auto_assign_source(&sources) {
        AutoAssignSelection::Source(source) => source.clone(),
        AutoAssignSelection::NoSource => {
            return json!({
                "assigned": false,
                "reason": "no host source allows automatic assignment",
            });
        }
        AutoAssignSelection::NoDefault => {
            return json!({
                "assigned": false,
                "reason": "multiple host sources allow automatic assignment but none is the default",
            });
        }
    };

    let service = HostBindingService::new(db, cache);
    let mut last_error: Option<AppError> = None;
    for attempt in 0..3u8 {
        let host = hosts::platform_host_candidate(&project.slug, &source.base_domain, attempt);
        match service
            .bind_host(
                OP,
                BindHostRequest {
                    project,
                    team,
                    source: Some(&source),
                    host,
                    kind: HostBindingKind::Platform,
                    environment: HostBindingEnvironment::Production,
                    is_primary: true,
                    review_status: HostReviewStatus::NotRequired,
                    actor_user_id: Some(session.data.user_id),
                },
            )
            .await
        {
            Ok(binding) => {
                let _ = audits::create_audit_event(
                    db,
                    CreateAuditEventParams {
                        actor_user_id: Some(session.data.user_id),
                        actor_node_id: None,
                        team_id: Some(team.id),
                        action: "host.provisioned".to_owned(),
                        target_type: "project_host_binding".to_owned(),
                        target_id: Some(binding.id),
                        result: AuditEventResult::Success,
                        reason: None,
                        metadata: json!({
                            "host": binding.host,
                            "status": format!("{:?}", binding.status).to_lowercase(),
                        }),
                    },
                )
                .await;
                return json!({
                    "assigned": true,
                    "host": binding.host,
                    "status": binding_status(&binding.status),
                    "failure_reason": binding.failure_reason,
                });
            }
            Err(AppError::Conflict { .. }) => continue,
            Err(AppError::QuotaExceeded { message, .. }) => {
                return json!({ "assigned": false, "reason": message });
            }
            Err(error) => {
                last_error = Some(error);
                break;
            }
        }
    }

    if let Some(error) = last_error {
        tracing::warn!(operation = OP, error = %error, "automatic host assignment failed");
        json!({ "assigned": false, "reason": "host provisioning failed; retry from the Hosts tab" })
    } else {
        json!({ "assigned": false, "reason": "could not find a free platform host" })
    }
}

fn binding_status(status: &crate::infra::database::entity::HostBindingStatus) -> &'static str {
    use crate::infra::database::entity::HostBindingStatus;
    match status {
        HostBindingStatus::Pending => "pending",
        HostBindingStatus::Active => "active",
        HostBindingStatus::Failed => "failed",
        HostBindingStatus::Disabled => "disabled",
    }
}
