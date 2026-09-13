use crate::infra::database::entity::NodeDeploymentMigrationStatus;

pub(crate) fn shadow_migration_statuses() -> [NodeDeploymentMigrationStatus; 3] {
    [
        NodeDeploymentMigrationStatus::Pending,
        NodeDeploymentMigrationStatus::Syncing,
        NodeDeploymentMigrationStatus::Ready,
    ]
}

pub(crate) fn migration_is_shadow_assignment(status: &NodeDeploymentMigrationStatus) -> bool {
    shadow_migration_statuses().contains(status)
}

pub(crate) fn migration_allows_artifact_download(status: &NodeDeploymentMigrationStatus) -> bool {
    migration_is_shadow_assignment(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ready_shadow_assignments_remain_authorized_until_atomic_cutover() {
        for status in [
            NodeDeploymentMigrationStatus::Pending,
            NodeDeploymentMigrationStatus::Syncing,
            NodeDeploymentMigrationStatus::Ready,
        ] {
            assert!(migration_is_shadow_assignment(&status));
            assert!(migration_allows_artifact_download(&status));
        }
        assert!(!migration_is_shadow_assignment(
            &NodeDeploymentMigrationStatus::Failed
        ));
        assert!(!migration_allows_artifact_download(
            &NodeDeploymentMigrationStatus::Failed
        ));
    }
}
