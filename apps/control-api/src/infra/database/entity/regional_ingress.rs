use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "regional_ingresses")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub region: String,
    pub hostname: String,
    pub enabled: bool,
    pub health_check_path: String,
    pub health_check_interval_seconds: i32,
    pub origin_host_preservation: bool,
    pub tls_enabled: bool,
    pub certificate_issuer: String,
    pub certificate_auto_renew: bool,
    pub certificate_status: String,
    pub certificate_expires_at: Option<TimeDateTimeWithTimeZone>,
    pub certificate_error: Option<String>,
    pub dns_challenge_provider: Option<String>,
    pub dns_challenge_config: Json,
    pub dns_challenge_status: String,
    pub dns_challenge_record_name: Option<String>,
    pub dns_challenge_record_value: Option<String>,
    pub deleted_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
