use sea_orm::entity::prelude::*;

use super::enums::{
    DeploymentBuildStatus, DeploymentEnvironment, DeploymentReleaseStatus, DeploymentServeStatus,
    ProjectRuntime,
};

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "deployments")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub team_id: Uuid,
    pub build_node_id: Option<Uuid>,
    pub serve_node_id: Option<Uuid>,
    pub environment: DeploymentEnvironment,
    pub runtime_kind: ProjectRuntime,
    pub build_status: DeploymentBuildStatus,
    pub serve_status: DeploymentServeStatus,
    pub release_status: DeploymentReleaseStatus,
    pub serve_cpu_millicores: i64,
    pub serve_memory_mb: i64,
    pub serve_disk_mb: i64,
    pub overcommitted: bool,
    pub source_repository_url: Option<String>,
    pub source_credential_version_id: Option<Uuid>,
    pub source_branch: Option<String>,
    pub commit_hash: Option<String>,
    pub commit_message: Option<String>,
    pub triggered_by_user_id: Option<Uuid>,
    pub install_command: Option<String>,
    pub build_command: Option<String>,
    pub output_directory: Option<String>,
    pub source_metadata: Json,
    pub preview_host: Option<String>,
    pub build_stage: Option<String>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub serve_failure_code: Option<String>,
    pub serve_failure_message: Option<String>,
    pub claimed_at: Option<TimeDateTimeWithTimeZone>,
    pub build_started_at: Option<TimeDateTimeWithTimeZone>,
    pub build_finished_at: Option<TimeDateTimeWithTimeZone>,
    pub serve_started_at: Option<TimeDateTimeWithTimeZone>,
    pub serve_finished_at: Option<TimeDateTimeWithTimeZone>,
    pub deleted_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Project,
    #[sea_orm(
        belongs_to = "super::team::Entity",
        from = "Column::TeamId",
        to = "super::team::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Team,
    #[sea_orm(
        belongs_to = "super::node::Entity",
        from = "Column::BuildNodeId",
        to = "super::node::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    BuildNode,
    #[sea_orm(
        belongs_to = "super::node::Entity",
        from = "Column::ServeNodeId",
        to = "super::node::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    ServeNode,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl Related<super::team::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Team.def()
    }
}

impl Related<super::node::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::BuildNode.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
