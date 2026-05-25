use sea_orm::entity::prelude::*;

use super::enums::HostSourceKind;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "host_sources")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub kind: HostSourceKind,
    pub label: String,
    pub base_domain: String,
    pub enabled: bool,
    pub allows_auto_assign: bool,
    pub is_default: bool,
    pub provider: Option<String>,
    pub config: Json,
    pub deleted_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
