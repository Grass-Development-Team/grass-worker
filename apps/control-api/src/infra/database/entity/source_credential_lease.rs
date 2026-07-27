use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "source_credential_leases")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub token_hash: String,
    pub node_id: Uuid,
    pub deployment_id: Uuid,
    pub credential_version_id: Uuid,
    pub expires_at: TimeDateTimeWithTimeZone,
    pub consumed_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::deployment::Entity",
        from = "Column::DeploymentId",
        to = "super::deployment::Column::Id",
        on_delete = "Cascade"
    )]
    Deployment,
    #[sea_orm(
        belongs_to = "super::node::Entity",
        from = "Column::NodeId",
        to = "super::node::Column::Id",
        on_delete = "Cascade"
    )]
    Node,
    #[sea_orm(
        belongs_to = "super::source_credential_version::Entity",
        from = "Column::CredentialVersionId",
        to = "super::source_credential_version::Column::Id"
    )]
    CredentialVersion,
}

impl ActiveModelBehavior for ActiveModel {}
