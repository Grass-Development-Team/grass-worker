use sea_orm::entity::prelude::*;

use super::enums::{DeploymentEnvironment, ReleaseReason};

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "releases")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub deployment_id: Uuid,
    pub environment: DeploymentEnvironment,
    pub reason: ReleaseReason,
    pub actor_user_id: Option<Uuid>,
    pub previous_deployment_id: Option<Uuid>,
    pub created_at: TimeDateTimeWithTimeZone,
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
        belongs_to = "super::deployment::Entity",
        from = "Column::DeploymentId",
        to = "super::deployment::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Deployment,
}
impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}
impl Related<super::deployment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Deployment.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
