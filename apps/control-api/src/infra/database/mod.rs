pub mod connection;
pub mod entity;
pub mod migrate;
pub mod migration;
pub mod seed;

pub use connection::connect;

pub fn is_unique_violation(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<sea_orm::DbErr>()
        .and_then(sea_orm::DbErr::sql_err)
        .is_some_and(|error| matches!(error, sea_orm::SqlErr::UniqueConstraintViolation(_)))
}
