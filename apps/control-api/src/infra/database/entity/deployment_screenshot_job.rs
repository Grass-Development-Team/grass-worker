use sea_orm::entity::prelude::*;

use super::enums::DeploymentScreenshotStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "deployment_screenshot_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub deployment_id: Uuid,
    pub status: DeploymentScreenshotStatus,
    pub attempt_count: i32,
    pub next_attempt_at: TimeDateTimeWithTimeZone,
    pub last_error: Option<String>,
    pub artifact_id: Option<Uuid>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::deployment::Entity",
        from = "Column::DeploymentId",
        to = "super::deployment::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Deployment,
    #[sea_orm(
        belongs_to = "super::deployment_artifact::Entity",
        from = "Column::ArtifactId",
        to = "super::deployment_artifact::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Artifact,
}

impl Related<super::deployment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Deployment.def()
    }
}

impl Related<super::deployment_artifact::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Artifact.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
