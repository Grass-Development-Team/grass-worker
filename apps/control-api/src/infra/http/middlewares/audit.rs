use std::{net::SocketAddr, time::Instant};

use axum::{
    body::Body,
    extract::{ConnectInfo, MatchedPath, OriginalUri, State},
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::Response,
};
use sea_orm::EntityTrait;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::audits::{self, CreateRequestAuditEventParams},
    infra::{
        database::entity::{AuditEventResult, deployment, project},
        error::AuditErrorContext,
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

const REQUEST_ID_HEADER: &str = "x-request-id";

pub async fn audit_middleware(
    State(state): State<ControlApiState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let path = request
        .extensions()
        .get::<OriginalUri>()
        .map(|uri| uri.0.path().to_owned())
        .unwrap_or_else(|| request.uri().path().to_owned());
    if !should_audit_request(&path) {
        return next.run(request).await;
    }

    let request_id = Uuid::now_v7();
    let occurred_at = OffsetDateTime::now_utc();
    let started = Instant::now();
    let method = request.method().clone();
    let matched_path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned());
    let actor_user_id = request
        .extensions()
        .get::<Option<(String, grass_session::SessionData)>>()
        .and_then(|session| session.as_ref().map(|(_, data)| data.user_id));
    let source_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip().to_string());
    let user_agent = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| truncate(value, 512));
    let target = extract_request_target(&path);

    let mut response = next.run(request).await;
    let status = response.status();
    let result = result_for_status(status);
    let error_context = response.extensions().get::<AuditErrorContext>().cloned();
    let actor_node_id = response
        .extensions()
        .get::<AuthenticatedNode>()
        .map(|authenticated| authenticated.0.id);
    let action = error_context
        .as_ref()
        .map(|context| context.operation.to_owned())
        .unwrap_or_else(|| request_action(&method, matched_path.as_deref(), &path));
    let reason = error_context.map(|context| truncate(&context.reason, 2_048));
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(&request_id.to_string()).expect("UUID is a valid header value"),
    );

    if let Some(db) = state.try_database() {
        let (team_id, project_id) = resolve_target_context(db, &target).await;
        let metadata = json!({
            "matched_path": matched_path,
            "project_id": project_id,
        });
        if let Err(error) = audits::create_request_audit_event(
            db,
            CreateRequestAuditEventParams {
                request_id,
                actor_user_id,
                actor_node_id,
                team_id,
                action,
                target_type: target.target_type,
                target_id: target.target_id,
                result,
                reason,
                source_ip,
                user_agent,
                http_method: method.to_string(),
                request_path: path,
                status_code: status.as_u16(),
                duration_ms,
                changes: json!({}),
                metadata,
                occurred_at,
            },
        )
        .await
        {
            tracing::warn!(
                operation = "audit.request.write",
                %request_id,
                %error,
                "failed to record request audit event"
            );
        }
    }

    response
}

async fn resolve_target_context(
    db: &sea_orm::DatabaseConnection,
    target: &RequestTarget,
) -> (Option<Uuid>, Option<Uuid>) {
    let mut team_id = target.team_id;
    let mut project_id = target.project_id;

    if project_id.is_none() && target.target_type == "deployment" {
        let Some(deployment_id) = target.target_id else {
            return (team_id, project_id);
        };
        match deployment::Entity::find_by_id(deployment_id).one(db).await {
            Ok(Some(deployment)) => {
                team_id = Some(deployment.team_id);
                project_id = Some(deployment.project_id);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    operation = "audit.request.resolve_deployment",
                    %deployment_id,
                    %error,
                    "failed to resolve request audit deployment scope"
                );
            }
        }
    }

    if team_id.is_none()
        && let Some(resolved_project_id) = project_id
    {
        match project::Entity::find_by_id(resolved_project_id)
            .one(db)
            .await
        {
            Ok(Some(project)) => team_id = Some(project.team_id),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    operation = "audit.request.resolve_team",
                    project_id = %resolved_project_id,
                    %error,
                    "failed to resolve request audit team scope"
                );
            }
        }
    }

    (team_id, project_id)
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub fn should_audit_request(path: &str) -> bool {
    if !path.starts_with("/api/v1/") {
        return false;
    }

    if matches!(
        path,
        "/api/v1/internal/nodes/heartbeat"
            | "/api/v1/internal/serve/assignments"
            | "/api/v1/internal/serve/routes"
            | "/api/v1/internal/serve/resolve-host"
    ) {
        return false;
    }

    if path.starts_with("/api/v1/internal/deployments/")
        && ["/build-log", "/static-site", "/artifact"]
            .iter()
            .any(|suffix| path.ends_with(suffix))
    {
        return false;
    }

    true
}

pub fn result_for_status(status: StatusCode) -> AuditEventResult {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
            AuditEventResult::Denied
        }
        status if status.is_success() || status.is_redirection() => AuditEventResult::Success,
        _ => AuditEventResult::Failure,
    }
}

pub fn request_action(method: &Method, matched_path: Option<&str>, path: &str) -> String {
    let route = matched_path.unwrap_or(path);
    format!(
        "api.request.{} {route}",
        method.as_str().to_ascii_lowercase()
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestTarget {
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
}

pub fn extract_request_target(path: &str) -> RequestTarget {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut target = RequestTarget {
        target_type: "api_route".to_owned(),
        target_id: None,
        team_id: None,
        project_id: None,
    };

    for pair in segments.windows(2) {
        let Some(target_type) = resource_type(pair[0]) else {
            continue;
        };
        let Ok(id) = pair[1].parse::<Uuid>() else {
            continue;
        };

        if target_type == "team" {
            target.team_id = Some(id);
        } else if target_type == "project" {
            target.project_id = Some(id);
        }
        target.target_type = target_type.to_owned();
        target.target_id = Some(id);
    }

    if target.target_id.is_none() {
        target.target_type = collection_target_type(&segments).to_owned();
    }

    target
}

fn collection_target_type(segments: &[&str]) -> &'static str {
    if segments.contains(&"auth") {
        "authentication"
    } else if segments.contains(&"setup") {
        "platform_setup"
    } else if segments.contains(&"me") {
        "user"
    } else if segments.contains(&"admin") {
        "platform"
    } else if segments.contains(&"projects") {
        "project"
    } else if segments.contains(&"teams") {
        "team"
    } else {
        "api_route"
    }
}

fn resource_type(segment: &str) -> Option<&'static str> {
    match segment {
        "teams" => Some("team"),
        "projects" => Some("project"),
        "deployments" => Some("deployment"),
        "nodes" => Some("node"),
        "users" => Some("user"),
        "team-groups" => Some("team_group"),
        "quota-plans" => Some("quota_plan"),
        "host-sources" => Some("host_source"),
        "hosts" => Some("host"),
        "source-credentials" => Some("source_credential"),
        "ssh-host-keys" => Some("ssh_host_key"),
        "invitations" => Some("invitation"),
        "reviews" => Some("review"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        extract::ConnectInfo,
        http::{Method, Request, StatusCode},
        middleware,
        response::IntoResponse,
        routing::get,
    };
    use tower::ServiceExt;

    use crate::{
        infra::{config::ControlApiConfig, database::entity::AuditEventResult, error::AppError},
        state::ControlApiState,
    };

    use super::{
        REQUEST_ID_HEADER, audit_middleware, extract_request_target, request_action,
        result_for_status, should_audit_request,
    };

    #[test]
    fn user_api_and_denial_paths_are_audited() {
        for path in [
            "/api/v1/auth/login",
            "/api/v1/me",
            "/api/v1/projects",
            "/api/v1/projects/0196/deployments/0197/logs/ws",
            "/api/v1/teams/0196/audit-events",
            "/api/v1/admin/settings",
            "/api/v1/internal/log-stream",
        ] {
            assert!(should_audit_request(path), "expected {path} to be audited");
        }
    }

    #[test]
    fn high_volume_machine_and_static_paths_are_not_audited() {
        for path in [
            "/health",
            "/assets/index.js",
            "/api/v1/internal/nodes/heartbeat",
            "/api/v1/internal/serve/assignments",
            "/api/v1/internal/serve/routes",
            "/api/v1/internal/serve/resolve-host",
            "/api/v1/internal/deployments/0196/build-log",
            "/api/v1/internal/deployments/0196/static-site",
            "/api/v1/internal/deployments/0196/artifact",
        ] {
            assert!(
                !should_audit_request(path),
                "expected {path} to be excluded from request audit"
            );
        }
    }

    #[test]
    fn audit_result_distinguishes_denials_from_failures() {
        assert_eq!(result_for_status(StatusCode::OK), AuditEventResult::Success);
        assert_eq!(
            result_for_status(StatusCode::UNAUTHORIZED),
            AuditEventResult::Denied
        );
        assert_eq!(
            result_for_status(StatusCode::FORBIDDEN),
            AuditEventResult::Denied
        );
        assert_eq!(
            result_for_status(StatusCode::TOO_MANY_REQUESTS),
            AuditEventResult::Denied
        );
        assert_eq!(
            result_for_status(StatusCode::BAD_REQUEST),
            AuditEventResult::Failure
        );
        assert_eq!(
            result_for_status(StatusCode::INTERNAL_SERVER_ERROR),
            AuditEventResult::Failure
        );
    }

    #[test]
    fn audit_action_includes_method_and_route_pattern() {
        assert_eq!(
            request_action(
                &Method::PATCH,
                Some("/api/v1/projects/{project_id}"),
                "/api/v1/projects/0196"
            ),
            "api.request.patch /api/v1/projects/{project_id}"
        );
    }

    #[test]
    fn request_target_uses_the_most_specific_resource() {
        let project_id = uuid::Uuid::parse_str("0196335e-9491-7df2-b7d5-2dca1db6559a").unwrap();
        let deployment_id = uuid::Uuid::parse_str("0196335e-a53e-70d1-8072-623128576a62").unwrap();
        let path = format!("/api/v1/projects/{project_id}/deployments/{deployment_id}/promote");

        let target = extract_request_target(&path);

        assert_eq!(target.target_type, "deployment");
        assert_eq!(target.target_id, Some(deployment_id));
        assert_eq!(target.project_id, Some(project_id));
        assert_eq!(target.team_id, None);
    }

    #[test]
    fn request_target_keeps_explicit_team_scope() {
        let team_id = uuid::Uuid::parse_str("0196335e-be2c-74df-b7aa-65c73f68c108").unwrap();
        let path = format!("/api/v1/teams/{team_id}/members");

        let target = extract_request_target(&path);

        assert_eq!(target.target_type, "team");
        assert_eq!(target.target_id, Some(team_id));
        assert_eq!(target.team_id, Some(team_id));
    }

    #[tokio::test]
    async fn audited_responses_receive_a_request_id_without_changing_status() {
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        let app = Router::new()
            .route("/api/v1/me", get(|| async { StatusCode::NO_CONTENT }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                audit_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let request_id = response
            .headers()
            .get("x-request-id")
            .expect("request id header")
            .to_str()
            .unwrap();
        assert!(uuid::Uuid::parse_str(request_id).is_ok());
    }

    #[tokio::test]
    async fn denied_user_request_persists_actor_source_and_team_context() {
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.database.set(db.clone()).is_ok());
        let team_id = uuid::Uuid::parse_str("0196335e-be2c-74df-b7aa-65c73f68c108").unwrap();
        let actor_id = uuid::Uuid::parse_str("0196335e-cab1-70f8-bd7e-ad493b503e57").unwrap();
        let app = Router::new()
            .route(
                "/api/v1/teams/{team_id}",
                get(|| async {
                    AppError::Forbidden {
                        op: "team.role.not_member",
                        message: "not a member of this team".to_owned(),
                    }
                    .into_response()
                }),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                audit_middleware,
            ))
            .with_state(state);
        let now = time::OffsetDateTime::UNIX_EPOCH;
        let mut request = Request::builder()
            .uri(format!("/api/v1/teams/{team_id}"))
            .header("user-agent", "Grass Console")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(Some((
            "session".to_owned(),
            grass_session::SessionData {
                user_id: actor_id,
                created_at: now,
                last_accessed_at: now,
            },
        )));
        request.extensions_mut().insert(ConnectInfo(
            "192.0.2.10:443".parse::<std::net::SocketAddr>().unwrap(),
        ));

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("INSERT INTO \\\"audit_events\\\""));
        assert!(statements.contains("team.role.not_member"));
        assert!(statements.contains("denied"));
        assert!(statements.contains(&actor_id.to_string()));
        assert!(statements.contains(&team_id.to_string()));
        assert!(statements.contains("192.0.2.10"));
    }

    #[tokio::test]
    async fn deployment_only_request_resolves_team_and_project_context() {
        use crate::infra::database::entity::{
            DeploymentBuildStatus, DeploymentEnvironment, DeploymentReleaseStatus,
            DeploymentServeStatus, ProjectRuntime, ReleaseReason, deployment,
        };

        let deployment_id = uuid::Uuid::now_v7();
        let project_id = uuid::Uuid::now_v7();
        let team_id = uuid::Uuid::now_v7();
        let now = time::OffsetDateTime::UNIX_EPOCH;
        let deployment = deployment::Model {
            id: deployment_id,
            project_id,
            team_id,
            build_node_id: None,
            serve_node_id: None,
            environment: DeploymentEnvironment::Production,
            runtime_kind: ProjectRuntime::Static,
            build_status: DeploymentBuildStatus::Ready,
            serve_status: DeploymentServeStatus::Pending,
            release_status: DeploymentReleaseStatus::Draft,
            serve_cpu_millicores: 0,
            serve_memory_mb: 0,
            serve_disk_mb: 0,
            overcommitted: false,
            source_repository_url: None,
            source_credential_version_id: None,
            source_branch: None,
            commit_hash: None,
            commit_message: None,
            triggered_by_user_id: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_metadata: serde_json::json!({}),
            preview_host: None,
            build_stage: None,
            failure_code: None,
            failure_message: None,
            serve_failure_code: None,
            serve_failure_message: None,
            pending_release_reason: None::<ReleaseReason>,
            pending_release_actor_user_id: None,
            pending_release_audit_visibility: None,
            pending_release_requested_at: None,
            claimed_at: None,
            build_started_at: None,
            build_finished_at: None,
            serve_started_at: None,
            serve_finished_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[deployment]])
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.database.set(db.clone()).is_ok());
        let app = Router::new()
            .route(
                "/api/v1/internal/deployments/{deployment_id}/stage",
                get(|| async { StatusCode::NO_CONTENT }),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                audit_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/internal/deployments/{deployment_id}/stage"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains(&team_id.to_string()), "{statements}");
        assert!(statements.contains(&project_id.to_string()), "{statements}");
    }

    #[tokio::test]
    async fn application_router_audits_authentication_rejections() {
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        let app = crate::features::router::router(state.clone()).with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(REQUEST_ID_HEADER));
    }
}
