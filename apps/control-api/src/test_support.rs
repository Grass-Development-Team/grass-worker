//! Shared fixtures for contract and integration tests.
use serde::{Serialize, de::DeserializeOwned};

pub(crate) fn assert_node_contract<
    Local: Serialize + DeserializeOwned,
    Protocol: Serialize + DeserializeOwned,
>(
    name: &str,
) {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/node-contracts.json")).unwrap();
    for example in fixtures[name].as_array().expect("known protocol fixture") {
        let local: Local = serde_json::from_value(example.clone()).unwrap();
        let protocol: Protocol = serde_json::from_value(example.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(local).unwrap(),
            serde_json::to_value(protocol).unwrap(),
            "wire contract: {name}"
        );
    }
    assert_eq!(
        serde_json::from_value::<Local>(serde_json::Value::Null).is_err(),
        serde_json::from_value::<Protocol>(serde_json::Value::Null).is_err(),
        "null contract: {name}"
    );
}

pub(crate) fn ready_deployment() -> crate::infra::database::entity::deployment::Model {
    use crate::infra::database::entity::{
        DeploymentBuildStatus, DeploymentEnvironment, DeploymentReleaseStatus,
        DeploymentServeStatus, ProjectRuntime,
    };
    use uuid::Uuid;
    crate::infra::database::entity::deployment::Model {
        id: Uuid::nil(),
        project_id: Uuid::nil(),
        team_id: Uuid::nil(),
        region: "default".to_owned(),
        build_node_id: None,
        serve_node_id: None,
        environment: DeploymentEnvironment::Production,
        runtime_kind: ProjectRuntime::Static,
        build_status: DeploymentBuildStatus::Ready,
        serve_status: DeploymentServeStatus::Ready,
        release_status: DeploymentReleaseStatus::Active,
        serve_cpu_millicores: 50,
        serve_memory_mb: 64,
        serve_disk_mb: 256,
        overcommitted: false,
        source_repository_url: Some("https://example.test/repo.git".to_owned()),
        source_credential_version_id: None,
        source_branch: None,
        commit_hash: None,
        commit_message: None,
        triggered_by_user_id: None,
        install_command: None,
        build_command: None,
        output_directory: None,
        source_metadata: serde_json::json!({}),
        preview_host: Some("preview.test".to_owned()),
        build_stage: None,
        failure_code: None,
        failure_message: None,
        serve_failure_code: None,
        serve_failure_message: None,
        pending_release_reason: None,
        pending_release_actor_user_id: None,
        pending_release_audit_visibility: None,
        pending_release_requested_at: None,
        claimed_at: None,
        build_started_at: Some(time::OffsetDateTime::UNIX_EPOCH),
        build_finished_at: Some(time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(5)),
        serve_started_at: None,
        serve_finished_at: None,
        deleted_at: None,
        created_at: time::OffsetDateTime::UNIX_EPOCH,
        updated_at: time::OffsetDateTime::UNIX_EPOCH,
    }
}
