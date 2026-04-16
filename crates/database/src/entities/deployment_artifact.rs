use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum ArtifactKind {
    #[sea_orm(string_value = "static_site")]
    StaticSite,
    #[sea_orm(string_value = "build_log")]
    BuildLog,
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "deployment_artifacts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub kind: ArtifactKind,
    pub storage_path: String,
    pub checksum_sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::deployment::Entity",
        from = "Column::DeploymentId",
        to = "super::deployment::Column::Id",
        on_update = "Cascade",
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
