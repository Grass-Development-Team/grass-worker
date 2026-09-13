pub(crate) mod by_project_id;

use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        host_bindings::{BindHostRequest, HostBindingService},
        hosts::{self, AutoAssignSelection},
        projects::{self, CreateProjectParams},
        quotas::QuotaDimension,
        teams,
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{
            AuditEventResult, HostBindingEnvironment, HostBindingKind, HostReviewStatus,
            ProjectRuntime, project,
        },
        error::{AppError, ok_response},
        http::extractors::Session,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/projects", axum::routing::get(list).post(create))
        .merge(by_project_id::router())
}

fn validate_repository_url(value: &str) -> Result<(), &'static str> {
    grass_git_source::parse_repository_url(value)
        .map(|_| ())
        .map_err(|error| match error {
            grass_git_source::RepositoryUrlError::UnsupportedTransport => {
                "repository_url transport is not supported"
            }
            grass_git_source::RepositoryUrlError::EmbeddedCredential => {
                "repository_url must not contain credentials"
            }
            grass_git_source::RepositoryUrlError::Invalid => "repository_url is invalid",
        })
}

fn project_view(project: &project::Model) -> ProjectResponse {
    ProjectResponse {
        id: project.id,
        team_id: project.team_id,
        slug: project.slug.clone(),
        name: project.name.clone(),
        runtime: projects::runtime_value(&project.runtime),
        repository_url: project.repository_url.clone(),
        default_branch: project.default_branch.clone(),
        install_command: project.install_command.clone(),
        build_command: project.build_command.clone(),
        output_directory: project.output_directory.clone(),
        source_config: project.source_config.clone(),
        build_config: project.build_config.clone(),
        archived_at: project.archived_at,
        deleted_at: project.deleted_at,
        created_at: project.created_at,
        updated_at: project.updated_at,
    }
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_owned();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

#[derive(Deserialize)]
struct CreateProjectRequest {
    team_id: Uuid,
    name: String,
    slug: String,
    #[serde(default = "default_runtime")]
    runtime: String,
    #[serde(default)]
    repository_url: Option<String>,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    root_directory: Option<String>,
    #[serde(default)]
    install_command: Option<String>,
    #[serde(default)]
    build_command: Option<String>,
    #[serde(default)]
    output_directory: Option<String>,
    #[serde(default)]
    framework_hint: Option<String>,
}

fn default_runtime() -> String {
    "static".to_owned()
}

/// POST /api/v1/projects
async fn create(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<CreateProjectRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.create";

    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;

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
            validate_repository_url(trimmed).map_err(|message| AppError::Validation {
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
        "root_directory": optional_trimmed(body.root_directory),
        "framework_hint": optional_trimmed(body.framework_hint),
    });

    let project = match async {
        let transaction = audits::AuditTransaction::begin(db).await?;
        let project = projects::create_project(
            &transaction,
            CreateProjectParams {
                team_id: team.id,
                created_by_user_id: Some(session.data.user_id),
                slug,
                name: body.name.trim().to_owned(),
                runtime,
                repository_url: optional_trimmed(body.repository_url),
                default_branch: optional_trimmed(body.default_branch),
                install_command: optional_trimmed(body.install_command),
                build_command: optional_trimmed(body.build_command),
                output_directory: optional_trimmed(body.output_directory),
                source_config,
                build_config: json!({}),
            },
        )
        .await?;
        audits::create_audit_event(
            &transaction,
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
        .await?;
        transaction.commit().await?;
        Ok::<_, anyhow::Error>(project)
    }
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

    // Platform-domain auto-assignment. Failures never fail project creation;
    // the response carries the reason so the Console can explain it.
    let host_assignment = auto_assign_host(&state, &session, &team, &project).await;

    Ok(ok_response(CreateResponse {
        project: project_view(&project),
        host_assignment,
    }))
}

async fn auto_assign_host(
    state: &ControlApiState,
    session: &Session,
    team: &crate::infra::database::entity::team::Model,
    project: &crate::infra::database::entity::project::Model,
) -> HostAssignmentResponse {
    const OP: &str = "projects.create.auto_assign_host";

    let (Ok(db), Ok(cache)) = (
        crate::infra::http::database(state, OP),
        crate::infra::http::cache(state, OP),
    ) else {
        return HostAssignmentResponse::unassigned("infrastructure unavailable");
    };

    let policy = match hosts::policy_for_team_group(db, team.group_id).await {
        Ok(policy) => policy,
        Err(error) => {
            tracing::warn!(operation = OP, %error, "failed to load host policy");
            return HostAssignmentResponse::unassigned("host policy unavailable");
        }
    };
    if let Some(policy) = &policy
        && !policy.allow_auto_assign
    {
        return HostAssignmentResponse::unassigned(
            "team group does not allow automatic host assignment",
        );
    }

    let sources = match hosts::list_sources(db).await {
        Ok(sources) => sources,
        Err(error) => {
            tracing::warn!(operation = OP, %error, "failed to list host sources");
            return HostAssignmentResponse::unassigned("host sources unavailable");
        }
    };

    let source = match hosts::select_auto_assign_source(&sources) {
        AutoAssignSelection::Source(source) => source.clone(),
        AutoAssignSelection::NoSource => {
            return HostAssignmentResponse::unassigned(
                "no host source allows automatic assignment",
            );
        }
        AutoAssignSelection::NoDefault => {
            return HostAssignmentResponse::unassigned(
                "multiple host sources allow automatic assignment but none is the default",
            );
        }
    };

    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let service = HostBindingService::new(db, cache, &platform_secret);
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
                    region: source.region.clone(),
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
                audits::observe_event(
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
                return HostAssignmentResponse::Assigned {
                    assigned: true,
                    host: binding.host,
                    status: binding_status(&binding.status),
                    failure_reason: binding.failure_reason,
                };
            }
            Err(AppError::Conflict { .. }) => continue,
            Err(AppError::QuotaExceeded { message, .. }) => {
                return HostAssignmentResponse::unassigned(message);
            }
            Err(error) => {
                last_error = Some(error);
                break;
            }
        }
    }

    if let Some(error) = last_error {
        tracing::warn!(operation = OP, error = %error, "automatic host assignment failed");
        HostAssignmentResponse::unassigned("host provisioning failed; retry from the Hosts tab")
    } else {
        HostAssignmentResponse::unassigned("could not find a free platform host")
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

#[derive(Deserialize)]
struct ListProjectsQuery {
    team_id: Uuid,
}

/// GET /api/v1/projects?team_id=...
async fn list(
    State(state): State<ControlApiState>,
    session: Session,
    Query(query): Query<ListProjectsQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.list";
    let db = crate::infra::http::database(&state, OP)?;

    teams::member_role(db, query.team_id, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Forbidden {
            op: OP,
            message: "not a member of this team".to_owned(),
        })?;

    let projects = projects::list_for_team(db, query.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(ListResponse {
        projects: projects.iter().map(project_view).collect::<Vec<_>>(),
    }))
}

#[derive(serde::Serialize)]
struct ProjectResponse {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    slug: String,
    name: String,
    runtime: &'static str,
    repository_url: Option<String>,
    default_branch: Option<String>,
    install_command: Option<String>,
    build_command: Option<String>,
    output_directory: Option<String>,
    source_config: serde_json::Value,
    build_config: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    archived_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    deleted_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ListResponse {
    projects: Vec<ProjectResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    project: ProjectResponse,
    host_assignment: HostAssignmentResponse,
}
#[derive(serde::Serialize)]
#[serde(untagged)]
enum HostAssignmentResponse {
    Assigned {
        assigned: bool,
        host: String,
        status: &'static str,
        failure_reason: Option<String>,
    },
    Unassigned {
        assigned: bool,
        reason: String,
    },
}
impl HostAssignmentResponse {
    fn unassigned(reason: impl Into<String>) -> Self {
        Self::Unassigned {
            assigned: false,
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_urls_support_all_approved_git_transports() {
        for value in [
            "http://example.com/repo.git",
            "https://example.com:8443/repo.git",
            "ssh://git@example.com:2222/repo.git",
            "git@example.com:repo.git",
            "git://example.com:19418/repo.git",
        ] {
            assert!(validate_repository_url(value).is_ok(), "rejected {value}");
        }
    }

    #[test]
    fn project_urls_return_stable_safe_validation_messages() {
        assert_eq!(
            validate_repository_url("file:///srv/repo"),
            Err("repository_url transport is not supported")
        );
        assert_eq!(
            validate_repository_url("https://token@example.com/repo.git"),
            Err("repository_url must not contain credentials")
        );
        assert_eq!(
            validate_repository_url("../repo"),
            Err("repository_url is invalid")
        );
    }
}
