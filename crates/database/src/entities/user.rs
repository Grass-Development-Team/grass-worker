use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub email: String,
    pub is_admin: bool,
    pub is_initial_admin: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::project::Entity")]
    Projects,
    #[sea_orm(has_one = "super::user_password_credential::Entity")]
    PasswordCredential,
    #[sea_orm(has_many = "super::user_session::Entity")]
    Sessions,
}

impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Projects.def()
    }
}

impl Related<super::user_password_credential::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PasswordCredential.def()
    }
}

impl Related<super::user_session::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sessions.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
