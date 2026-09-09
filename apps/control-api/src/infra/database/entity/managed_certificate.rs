use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "managed_certificates")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub ingress_id: Uuid,
    pub host_binding_id: Option<Uuid>,
    pub hostname: String,
    pub issuer: String,
    pub challenge_method: String,
    pub auto_renew: bool,
    pub status: String,
    pub error: Option<String>,
    pub bundle: Option<Json>,
    pub acme_account: Option<Json>,
    pub revision: String,
    pub issued_at: Option<TimeDateTimeWithTimeZone>,
    pub expires_at: Option<TimeDateTimeWithTimeZone>,
    pub retry_at: Option<TimeDateTimeWithTimeZone>,
    pub failure_count: i32,
    pub lease_until: Option<TimeDateTimeWithTimeZone>,
    pub generation: Uuid,
    pub challenge_token: Option<String>,
    pub challenge_value: Option<String>,
    pub challenge_expires_at: Option<TimeDateTimeWithTimeZone>,
    pub dns_record_name: Option<String>,
    pub dns_record_value: Option<String>,
    pub dns_cleanup: Option<Json>,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
