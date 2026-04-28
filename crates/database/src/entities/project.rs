use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum ProjectStatus {
    #[sea_orm(string_value = "active")]
    Active,
    #[sea_orm(string_value = "archived")]
    Archived,
    #[sea_orm(string_value = "soft_deleted")]
    SoftDeleted,
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "projects")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub slug: String,
    pub name: String,
    pub status: ProjectStatus,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub archived_at: Option<DateTimeUtc>,
    pub soft_deleted_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::OwnerUserId",
        to = "super::user::Column::Id",
        on_update = "Cascade",
        on_delete = "Restrict"
    )]
    Owner,
    #[sea_orm(has_many = "super::deployment::Entity")]
    Deployments,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Owner.def()
    }
}

impl Related<super::deployment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Deployments.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
