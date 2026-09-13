use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use sea_orm::MockDatabase;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    infra::{
        config::ControlApiConfig,
        database::entity::{
            PlatformRole, SystemSettingValueKind, TeamInvitationStatus, TeamKind, TeamMemberRole,
            UserStatus, system_setting, team, team_invitation, team_member, user,
        },
    },
    state::ControlApiState,
};

fn authenticated_request(uri: String, user_id: Uuid) -> Request<Body> {
    let now = OffsetDateTime::now_utc();
    let mut request = Request::builder().uri(uri).body(Body::empty()).unwrap();
    request.extensions_mut().insert(Some((
        "test-session".to_owned(),
        grass_session::SessionData {
            auth_version: 1,
            user_id,
            created_at: now,
            last_accessed_at: now,
        },
    )));
    request
}

fn admin_membership(team_id: Uuid, user_id: Uuid) -> team_member::Model {
    let now = OffsetDateTime::now_utc();
    team_member::Model {
        id: Uuid::now_v7(),
        team_id,
        user_id,
        role: TeamMemberRole::Admin,
        invited_by_user_id: None,
        joined_at: now,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn signup_policy(value: &str) -> system_setting::Model {
    let now = OffsetDateTime::now_utc();
    system_setting::Model {
        id: Uuid::now_v7(),
        key: "signup.policy".to_owned(),
        value_kind: SystemSettingValueKind::String,
        value: serde_json::json!(value),
        is_secret: false,
        created_at: now,
        updated_at: now,
    }
}

fn active_user(email: &str, display_name: Option<&str>) -> user::Model {
    let now = OffsetDateTime::now_utc();
    user::Model {
        auth_version: 1,
        id: Uuid::now_v7(),
        email: email.to_owned(),
        display_name: display_name.map(str::to_owned),
        avatar_version: None,
        status: UserStatus::Active,
        platform_role: PlatformRole::User,
        email_verified_at: Some(now),
        last_login_at: None,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn candidates_find_registered_users_even_when_registration_is_closed() {
    let team_id = Uuid::now_v7();
    let admin_id = Uuid::now_v7();
    let candidate = active_user("alice@example.com", Some("Alice"));
    let candidate_id = candidate.id;
    let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_query_results([[admin_membership(team_id, admin_id)]])
        .append_query_results([[signup_policy("closed")]])
        .append_query_results([[candidate]])
        .into_connection();
    let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
    assert!(state.database.set(database).is_ok());

    let response = crate::features::api::v1::teams::router()
        .merge(crate::features::api::v1::team_invitations::router())
        .with_state(state)
        .oneshot(authenticated_request(
            format!("/teams/{team_id}/invitation-candidates?q=Ali"),
            admin_id,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["data"]["candidates"][0]["kind"], "user");
    assert_eq!(
        body["data"]["candidates"][0]["user_id"],
        candidate_id.to_string()
    );
    assert_eq!(body["data"]["candidates"][0]["email"], "alice@example.com");
    assert_eq!(body["data"]["candidates"][0]["display_name"], "Alice");
}

#[tokio::test]
async fn candidates_offer_an_exact_unregistered_email_only_for_open_registration() {
    for (policy, expected_count) in [("open", 1), ("invite_only", 0)] {
        let team_id = Uuid::now_v7();
        let admin_id = Uuid::now_v7();
        let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[admin_membership(team_id, admin_id)]])
            .append_query_results([[signup_policy(policy)]])
            .append_query_results([Vec::<user::Model>::new()])
            .append_query_results([Vec::<user::Model>::new()])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.database.set(database).is_ok());

        let response = crate::features::api::v1::teams::router()
            .merge(crate::features::api::v1::team_invitations::router())
            .with_state(state)
            .oneshot(authenticated_request(
                format!("/teams/{team_id}/invitation-candidates?q=new-user%40example.com"),
                admin_id,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body["data"]["candidates"].as_array().unwrap().len(),
            expected_count,
            "policy {policy}"
        );
        if policy == "open" {
            assert_eq!(body["data"]["candidates"][0]["kind"], "email");
            assert_eq!(
                body["data"]["candidates"][0]["user_id"],
                serde_json::Value::Null
            );
            assert_eq!(
                body["data"]["candidates"][0]["email"],
                "new-user@example.com"
            );
        }
    }
}

#[tokio::test]
async fn candidates_include_an_exact_registered_email_beyond_the_fuzzy_limit() {
    let team_id = Uuid::now_v7();
    let admin_id = Uuid::now_v7();
    let exact_candidate = active_user("target@example.com", Some("Target"));
    let exact_candidate_id = exact_candidate.id;
    let fuzzy_candidates = (0..10)
        .map(|index| {
            active_user(
                &format!("match-{index}@example.com"),
                Some("target@example.com"),
            )
        })
        .collect::<Vec<_>>();
    let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_query_results([[admin_membership(team_id, admin_id)]])
        .append_query_results([[signup_policy("closed")]])
        .append_query_results([fuzzy_candidates])
        .append_query_results([[exact_candidate]])
        .into_connection();
    let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
    assert!(state.database.set(database).is_ok());

    let response = crate::features::api::v1::teams::router()
        .merge(crate::features::api::v1::team_invitations::router())
        .with_state(state)
        .oneshot(authenticated_request(
            format!("/teams/{team_id}/invitation-candidates?q=target%40example.com"),
            admin_id,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let candidates = body["data"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 10);
    assert_eq!(
        candidates
            .iter()
            .filter(|candidate| candidate["user_id"] == exact_candidate_id.to_string())
            .count(),
        1
    );
}

#[tokio::test]
async fn candidates_do_not_offer_a_disabled_exact_user_as_an_open_email() {
    let team_id = Uuid::now_v7();
    let admin_id = Uuid::now_v7();
    let mut disabled_candidate = active_user("disabled@example.com", Some("Disabled"));
    disabled_candidate.status = UserStatus::Disabled;
    let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_query_results([[admin_membership(team_id, admin_id)]])
        .append_query_results([[signup_policy("open")]])
        .append_query_results([Vec::<user::Model>::new()])
        .append_query_results([[disabled_candidate]])
        .into_connection();
    let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
    assert!(state.database.set(database).is_ok());

    let response = crate::features::api::v1::teams::router()
        .merge(crate::features::api::v1::team_invitations::router())
        .with_state(state)
        .oneshot(authenticated_request(
            format!("/teams/{team_id}/invitation-candidates?q=disabled%40example.com"),
            admin_id,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(body["data"]["candidates"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn preflight_is_public_and_describes_the_invitation() {
    let now = OffsetDateTime::now_utc();
    let team_id = Uuid::now_v7();
    let token = "invitation-secret";
    let invitation = team_invitation::Model {
        id: Uuid::now_v7(),
        team_id,
        email: "invitee@example.com".to_owned(),
        role: TeamMemberRole::Member,
        status: TeamInvitationStatus::Pending,
        invited_by_user_id: None,
        token_hash: Some(crate::domain::teams::invitation_token_hash(token)),
        expires_at: now + Duration::days(7),
        accepted_at: None,
        created_at: now,
        updated_at: now,
    };
    let team = team::Model {
        id: team_id,
        slug: "acme".to_owned(),
        name: "Acme Team".to_owned(),
        avatar_version: None,
        kind: TeamKind::Team,
        group_id: None,
        explicit_quota_plan_id: None,
        owner_user_id: None,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    };
    let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_query_results([[invitation]])
        .append_query_results([[team]])
        .into_connection();
    let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
    assert!(state.database.set(database).is_ok());

    let response = crate::features::api::v1::teams::router()
        .merge(crate::features::api::v1::team_invitations::router())
        .with_state(state)
        .oneshot(
            Request::builder()
                .uri(format!("/team-invitations/preflight?token={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["data"]["team"]["name"], "Acme Team");
    assert_eq!(body["data"]["role"], "member");
    assert_eq!(body["data"]["status"], "pending");
    assert_eq!(
        body["data"]["email_matches_current_user"],
        serde_json::Value::Null
    );
    assert_eq!(body["data"]["can_accept"], false);
}
