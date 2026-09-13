use sea_orm::{ColumnTrait, Condition, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{
    AuditActorType, AuditEventResult, AuditEventVisibility, audit_event,
};

#[derive(Default)]
pub struct AuditEventFilter {
    pub action: Option<String>,
    pub actor_user_id: Option<Uuid>,
    pub actor_type: Option<AuditActorType>,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    /// Restrict to one team's events; `None` is the platform-wide view.
    pub team_id: Option<Uuid>,
    pub result: Option<AuditEventResult>,
    pub created_from: Option<OffsetDateTime>,
    pub created_to: Option<OffsetDateTime>,
    pub visibility: Option<AuditEventVisibility>,
    /// Team audit is a curated business view, not a filtered platform log.
    pub team_visible_only: bool,
    pub page: u64,
    pub per_page: u64,
}

pub fn audit_event_condition(filter: &AuditEventFilter) -> Condition {
    let mut condition = Condition::all();
    if let Some(action) = filter.action.as_deref() {
        condition = condition.add(audit_event::Column::Action.starts_with(action));
    }
    if let Some(actor_user_id) = filter.actor_user_id {
        condition = condition.add(audit_event::Column::ActorUserId.eq(actor_user_id));
    }
    if let Some(actor_type) = filter.actor_type.as_ref() {
        condition = condition.add(audit_event::Column::ActorType.eq(actor_type.clone()));
    }
    if let Some(target_type) = filter.target_type.as_deref() {
        condition = condition.add(audit_event::Column::TargetType.eq(target_type));
    }
    if let Some(target_id) = filter.target_id {
        condition = condition.add(audit_event::Column::TargetId.eq(target_id));
    }
    if let Some(team_id) = filter.team_id {
        condition = condition.add(audit_event::Column::TeamId.eq(team_id));
    }
    if let Some(visibility) = filter.visibility.as_ref() {
        condition = condition.add(audit_event::Column::Visibility.eq(visibility.clone()));
    }
    if let Some(result) = filter.result.as_ref() {
        condition = condition.add(audit_event::Column::Result.eq(result.clone()));
    }
    if let Some(created_from) = filter.created_from {
        condition = condition.add(audit_event::Column::CreatedAt.gte(created_from));
    }
    if let Some(created_to) = filter.created_to {
        condition = condition.add(audit_event::Column::CreatedAt.lte(created_to));
    }
    if filter.team_visible_only {
        condition = condition.add(
            Condition::any()
                .add(audit_event::Column::Action.starts_with("deployment."))
                .add(audit_event::Column::Action.eq("artifact.uploaded"))
                .add(audit_event::Column::Action.starts_with("host."))
                .add(audit_event::Column::Action.starts_with("project."))
                .add(audit_event::Column::Action.starts_with("quota."))
                .add(audit_event::Column::Action.starts_with("team.member."))
                .add(audit_event::Column::Action.starts_with("team.invitation.")),
        );
    }

    condition
}

pub fn audit_event_query(filter: &AuditEventFilter) -> sea_orm::Select<audit_event::Entity> {
    audit_event::Entity::find().filter(audit_event_condition(filter))
}

pub struct AuditEventPage {
    pub events: Vec<audit_event::Model>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
    pub total_pages: u64,
}

pub async fn list_events<C: ConnectionTrait>(
    db: &C,
    filter: AuditEventFilter,
) -> anyhow::Result<AuditEventPage> {
    let page = filter.page.max(1);
    let per_page = match filter.per_page {
        0 => 50,
        value => value.clamp(1, 100),
    };
    let query = audit_event_query(&filter);
    paginate_events(db, query, page, per_page).await
}

async fn paginate_events<C: ConnectionTrait>(
    db: &C,
    query: sea_orm::Select<audit_event::Entity>,
    page: u64,
    per_page: u64,
) -> anyhow::Result<AuditEventPage> {
    use sea_orm::{QueryOrder, QuerySelect};
    let total = query.clone().count(db).await?;
    let events = query
        .order_by_desc(audit_event::Column::CreatedAt)
        .order_by_desc(audit_event::Column::Id)
        .offset((page - 1).saturating_mul(per_page))
        .limit(per_page)
        .all(db)
        .await?;

    Ok(AuditEventPage {
        events,
        page,
        per_page,
        total,
        total_pages: total.div_ceil(per_page),
    })
}

pub async fn delete_events<C: ConnectionTrait>(
    db: &C,
    filter: AuditEventFilter,
) -> anyhow::Result<u64> {
    audit_event::Entity::delete_many()
        .filter(audit_event_condition(&filter))
        .exec(db)
        .await
        .map(|result| result.rows_affected)
        .map_err(Into::into)
}

/// Platform project activity, including the project's deployment and host targets.
/// The caller authorizes access and resolves the IDs belonging to this project.
pub(crate) async fn list_project_activity<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    deployment_ids: Vec<Uuid>,
    binding_ids: Vec<Uuid>,
    page: u64,
    per_page: u64,
) -> anyhow::Result<AuditEventPage> {
    let query = audit_event::Entity::find().filter(project_activity_condition(
        project_id,
        deployment_ids,
        binding_ids,
    ));
    paginate_events(db, query, page.max(1), per_page.clamp(1, 100)).await
}

fn project_activity_condition(
    project_id: Uuid,
    deployment_ids: Vec<Uuid>,
    binding_ids: Vec<Uuid>,
) -> Condition {
    let mut targets = Condition::any().add(
        Condition::all()
            .add(audit_event::Column::TargetType.eq("project"))
            .add(audit_event::Column::TargetId.eq(project_id)),
    );
    if !deployment_ids.is_empty() {
        targets = targets.add(
            Condition::all()
                .add(audit_event::Column::TargetType.eq("deployment"))
                .add(audit_event::Column::TargetId.is_in(deployment_ids)),
        );
    }
    if !binding_ids.is_empty() {
        targets = targets.add(
            Condition::all()
                .add(audit_event::Column::TargetType.is_in(["host", "project_host_binding"]))
                .add(audit_event::Column::TargetId.is_in(binding_ids)),
        );
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, MockDatabase, QueryTrait};
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn project_activity_keeps_legacy_host_targets_and_scoped_pagination() {
        let project_id = Uuid::from_u128(1);
        let deployment_id = Uuid::from_u128(2);
        let binding_id = Uuid::from_u128(3);
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([vec![BTreeMap::from([(
                "num_items",
                sea_orm::Value::BigInt(Some(3)),
            )])]])
            .append_query_results([Vec::<audit_event::Model>::new()])
            .into_connection();
        let page =
            list_project_activity(&db, project_id, vec![deployment_id], vec![binding_id], 2, 2)
                .await
                .unwrap();
        assert_eq!(
            (page.page, page.per_page, page.total, page.total_pages),
            (2, 2, 3, 2)
        );
        let sql = audit_event::Entity::find()
            .filter(project_activity_condition(
                project_id,
                vec![deployment_id],
                vec![binding_id],
            ))
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            sql.contains("'project'") && sql.contains(&project_id.to_string()),
            "{sql}"
        );
        assert!(
            sql.contains("'deployment'") && sql.contains(&deployment_id.to_string()),
            "{sql}"
        );
        assert!(
            sql.contains("'host', 'project_host_binding'") && sql.contains(&binding_id.to_string()),
            "{sql}"
        );
        let log = format!("{:?}", db.into_transaction_log());
        assert!(
            log.contains("ORDER BY") && log.contains("LIMIT") && log.contains("OFFSET"),
            "{log}"
        );
    }
}
