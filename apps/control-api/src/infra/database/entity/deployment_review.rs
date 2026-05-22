use sea_orm::entity::prelude::*;

use super::enums::DeploymentReviewStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "deployment_reviews")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub reviewer_user_id: Option<Uuid>,
    pub status: DeploymentReviewStatus,
    pub reason: Option<String>,
    pub requested_at: TimeDateTimeWithTimeZone,
    pub reviewed_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
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
}
impl Related<super::deployment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Deployment.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
