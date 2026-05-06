use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum PlatformHostSourceKind {
    #[sea_orm(string_value = "wildcard_static")]
    WildcardStatic,
    #[sea_orm(string_value = "dns_managed")]
    DnsManaged,
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "platform_host_sources")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub kind: PlatformHostSourceKind,
    pub label: String,
    pub base_domain: String,
    pub enabled: bool,
    pub allows_auto_assign: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::project_host_binding::Entity")]
    ProjectHostBindings,
}

impl Related<super::project_host_binding::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProjectHostBindings.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
