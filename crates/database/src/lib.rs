pub mod entities;

#[cfg(test)]
mod tests {
    use super::entities::{deployment, deployment_artifact, project};
    use sea_orm::EntityName;

    #[test]
    fn project_entity_uses_expected_table_name() {
        assert_eq!(project::Entity.table_name(), "projects");
    }

    #[test]
    fn deployment_entity_uses_expected_table_name() {
        assert_eq!(deployment::Entity.table_name(), "deployments");
    }

    #[test]
    fn deployment_artifact_entity_uses_expected_table_name() {
        assert_eq!(deployment_artifact::Entity.table_name(), "deployment_artifacts");
    }
}
