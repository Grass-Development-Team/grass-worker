use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "domain_onboarding")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub binding_id: Uuid,
    pub created_by_user_id: Option<Uuid>,
    pub contact_email: String,
    pub dns_status: String,
    pub dns_error: Option<String>,
    pub checked_at: Option<TimeDateTimeWithTimeZone>,
    pub next_check_at: TimeDateTimeWithTimeZone,
    pub lease_until: Option<TimeDateTimeWithTimeZone>,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
