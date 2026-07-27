use sea_orm::entity::prelude::*;

use super::enums::SshHostKeyStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "ssh_host_keys")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub host: String,
    pub port: i32,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint_sha256: String,
    pub status: SshHostKeyStatus,
    pub first_seen_node_id: Option<Uuid>,
    pub approved_by_user_id: Option<Uuid>,
    pub approved_at: Option<TimeDateTimeWithTimeZone>,
    pub last_seen_at: TimeDateTimeWithTimeZone,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
