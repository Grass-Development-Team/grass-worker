use crate::infra::database::entity::audit_event;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use time::OffsetDateTime;

pub async fn prune_events_before<C: ConnectionTrait>(
    db: &C,
    cutoff: OffsetDateTime,
) -> anyhow::Result<u64> {
    audit_event::Entity::delete_many()
        .filter(audit_event::Column::CreatedAt.lt(cutoff))
        .exec(db)
        .await
        .map(|result| result.rows_affected)
        .map_err(Into::into)
}
