//! Database-backed host source, host binding, and provision event functions.

use sea_orm::sea_query::LockType;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{
    HostBindingEnvironment, HostBindingKind, HostBindingStatus, HostProvisionEventStatus,
    HostReviewStatus, HostSourceKind, host_policy, host_provision_event, host_source,
    project_host_binding, team, team_group,
};

// --- Host sources -----------------------------------------------------------

pub struct CreateHostSourceParams {
    pub kind: HostSourceKind,
    pub label: String,
    pub base_domain: String,
    pub enabled: bool,
    pub allows_auto_assign: bool,
    pub is_default: bool,
    pub provider: Option<String>,
    pub config: serde_json::Value,
}

pub struct UpdateHostSourceParams {
    pub label: Option<String>,
    pub enabled: Option<bool>,
    pub allows_auto_assign: Option<bool>,
    pub is_default: Option<bool>,
    pub provider: Option<Option<String>>,
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum HostSourceError {
    #[error("only one enabled default host source is allowed")]
    DuplicateDefault,
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
}

pub async fn list_sources<C: ConnectionTrait>(db: &C) -> anyhow::Result<Vec<host_source::Model>> {
    host_source::Entity::find()
        .filter(host_source::Column::DeletedAt.is_null())
        .order_by_asc(host_source::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

pub async fn get_source_by_id<C: ConnectionTrait>(
    db: &C,
    source_id: Uuid,
) -> anyhow::Result<Option<host_source::Model>> {
    host_source::Entity::find()
        .filter(host_source::Column::Id.eq(source_id))
        .filter(host_source::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn get_source_by_id_including_deleted<C: ConnectionTrait>(
    db: &C,
    source_id: Uuid,
) -> anyhow::Result<Option<host_source::Model>> {
    host_source::Entity::find()
        .filter(host_source::Column::Id.eq(source_id))
        .one(db)
        .await
        .map_err(Into::into)
}

async fn default_source_exists<C: ConnectionTrait>(
    db: &C,
    exclude: Option<Uuid>,
) -> Result<bool, sea_orm::DbErr> {
    let mut query = host_source::Entity::find()
        .filter(host_source::Column::IsDefault.eq(true))
        .filter(host_source::Column::Enabled.eq(true))
        .filter(host_source::Column::DeletedAt.is_null());
    if let Some(exclude) = exclude {
        query = query.filter(host_source::Column::Id.ne(exclude));
    }
    query.one(db).await.map(|source| source.is_some())
}

pub async fn create_source<C: ConnectionTrait>(
    db: &C,
    params: CreateHostSourceParams,
) -> Result<host_source::Model, HostSourceError> {
    if params.is_default && params.enabled && default_source_exists(db, None).await? {
        return Err(HostSourceError::DuplicateDefault);
    }

    let now = OffsetDateTime::now_utc();
    host_source::ActiveModel {
        id: Set(Uuid::now_v7()),
        kind: Set(params.kind),
        label: Set(params.label),
        base_domain: Set(params.base_domain),
        enabled: Set(params.enabled),
        allows_auto_assign: Set(params.allows_auto_assign),
        is_default: Set(params.is_default),
        provider: Set(params.provider),
        config: Set(params.config),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(Into::into)
}

pub async fn update_source<C: ConnectionTrait>(
    db: &C,
    source: host_source::Model,
    params: UpdateHostSourceParams,
) -> Result<host_source::Model, HostSourceError> {
    let will_be_default = params.is_default.unwrap_or(source.is_default);
    let will_be_enabled = params.enabled.unwrap_or(source.enabled);
    if will_be_default && will_be_enabled && default_source_exists(db, Some(source.id)).await? {
        return Err(HostSourceError::DuplicateDefault);
    }

    let mut active: host_source::ActiveModel = source.into();
    if let Some(label) = params.label {
        active.label = Set(label);
    }
    if let Some(enabled) = params.enabled {
        active.enabled = Set(enabled);
    }
    if let Some(allows_auto_assign) = params.allows_auto_assign {
        active.allows_auto_assign = Set(allows_auto_assign);
    }
    if let Some(is_default) = params.is_default {
        active.is_default = Set(is_default);
    }
    if let Some(provider) = params.provider {
        active.provider = Set(provider);
    }
    if let Some(config) = params.config {
        active.config = Set(config);
    }
    active.update(db).await.map_err(Into::into)
}

pub async fn soft_delete_source<C: ConnectionTrait>(
    db: &C,
    source: host_source::Model,
) -> anyhow::Result<()> {
    let mut active: host_source::ActiveModel = source.into();
    active.deleted_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(db).await?;
    Ok(())
}

/// Selects the host source used for automatic platform-domain assignment.
///
/// Rules: only enabled sources that allow auto-assign qualify. A single
/// candidate is used directly; multiple candidates require exactly one
/// default source, otherwise assignment is skipped.
pub fn select_auto_assign_source(sources: &[host_source::Model]) -> AutoAssignSelection<'_> {
    let candidates: Vec<&host_source::Model> = sources
        .iter()
        .filter(|source| source.enabled && source.allows_auto_assign)
        .collect();

    match candidates.len() {
        0 => AutoAssignSelection::NoSource,
        1 => AutoAssignSelection::Source(candidates[0]),
        _ => {
            let defaults: Vec<&&host_source::Model> = candidates
                .iter()
                .filter(|source| source.is_default)
                .collect();
            match defaults.len() {
                1 => AutoAssignSelection::Source(defaults[0]),
                _ => AutoAssignSelection::NoDefault,
            }
        }
    }
}

pub enum AutoAssignSelection<'a> {
    Source(&'a host_source::Model),
    NoSource,
    NoDefault,
}

// --- Domain review policy --------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DomainReviewMode {
    Manual,
    #[default]
    Auto,
}

fn parse_domain_review_mode(
    value: Option<&serde_json::Value>,
    default: DomainReviewMode,
) -> DomainReviewMode {
    match value.and_then(serde_json::Value::as_str) {
        Some("manual") => DomainReviewMode::Manual,
        Some("auto") => DomainReviewMode::Auto,
        _ => default,
    }
}

fn apply_domain_review_policy_override(
    default: DomainReviewMode,
    policy: Option<&serde_json::Value>,
) -> DomainReviewMode {
    parse_domain_review_mode(policy.and_then(|value| value.get("domain")), default)
}

pub async fn domain_review_policy<C: ConnectionTrait>(db: &C) -> anyhow::Result<DomainReviewMode> {
    let default = DomainReviewMode::default();
    let setting = crate::domain::settings::get_setting(db, "domain_review_policy.default").await?;
    Ok(parse_domain_review_mode(
        setting.as_ref().map(|setting| &setting.value),
        default,
    ))
}

/// Resolves custom-domain review as Team Group override > platform default.
pub async fn domain_review_policy_for_team<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
) -> anyhow::Result<DomainReviewMode> {
    let default = domain_review_policy(db).await?;
    let team = team::Entity::find_by_id(team_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("team not found while resolving domain review policy"))?;
    let Some(group_id) = team.group_id else {
        return Ok(default);
    };
    let group = team_group::Entity::find_by_id(group_id)
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    Ok(apply_domain_review_policy_override(
        default,
        group
            .as_ref()
            .and_then(|group| group.review_policy.as_ref()),
    ))
}

// --- Host bindings ----------------------------------------------------------

pub struct CreateBindingParams {
    pub project_id: Uuid,
    pub team_id: Uuid,
    pub host_source_id: Option<Uuid>,
    pub host: String,
    pub kind: HostBindingKind,
    pub environment: HostBindingEnvironment,
    pub status: HostBindingStatus,
    pub failure_reason: Option<String>,
    pub is_primary: bool,
    pub review_status: HostReviewStatus,
}

pub async fn list_bindings_for_project<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> anyhow::Result<Vec<project_host_binding::Model>> {
    project_host_binding::Entity::find()
        .filter(project_host_binding::Column::ProjectId.eq(project_id))
        .filter(project_host_binding::Column::DeletedAt.is_null())
        .order_by_asc(project_host_binding::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

pub async fn list_bindings_for_project_for_update<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> anyhow::Result<Vec<project_host_binding::Model>> {
    project_host_binding::Entity::find()
        .filter(project_host_binding::Column::ProjectId.eq(project_id))
        .filter(project_host_binding::Column::DeletedAt.is_null())
        .order_by_asc(project_host_binding::Column::CreatedAt)
        .lock(LockType::Update)
        .all(db)
        .await
        .map_err(Into::into)
}

pub async fn get_binding_by_id<C: ConnectionTrait>(
    db: &C,
    binding_id: Uuid,
) -> anyhow::Result<Option<project_host_binding::Model>> {
    project_host_binding::Entity::find()
        .filter(project_host_binding::Column::Id.eq(binding_id))
        .filter(project_host_binding::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn get_binding_by_id_for_update<C: ConnectionTrait>(
    db: &C,
    binding_id: Uuid,
) -> anyhow::Result<Option<project_host_binding::Model>> {
    project_host_binding::Entity::find()
        .filter(project_host_binding::Column::Id.eq(binding_id))
        .filter(project_host_binding::Column::DeletedAt.is_null())
        .lock(LockType::Update)
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn get_binding_by_id_for_update_including_deleted<C: ConnectionTrait>(
    db: &C,
    binding_id: Uuid,
) -> anyhow::Result<Option<project_host_binding::Model>> {
    project_host_binding::Entity::find()
        .filter(project_host_binding::Column::Id.eq(binding_id))
        .lock(LockType::Update)
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn find_binding_by_host<C: ConnectionTrait>(
    db: &C,
    host: &str,
) -> anyhow::Result<Option<project_host_binding::Model>> {
    project_host_binding::Entity::find()
        .filter(project_host_binding::Column::Host.eq(host))
        .filter(project_host_binding::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn create_binding<C: ConnectionTrait>(
    db: &C,
    params: CreateBindingParams,
) -> anyhow::Result<project_host_binding::Model> {
    let now = OffsetDateTime::now_utc();
    project_host_binding::ActiveModel {
        id: Set(Uuid::now_v7()),
        project_id: Set(params.project_id),
        team_id: Set(params.team_id),
        host_source_id: Set(params.host_source_id),
        host: Set(params.host),
        kind: Set(params.kind),
        environment: Set(params.environment),
        status: Set(params.status),
        failure_reason: Set(params.failure_reason),
        is_primary: Set(params.is_primary),
        review_status: Set(params.review_status),
        reviewed_by_user_id: Set(None),
        reviewed_at: Set(None),
        review_reason: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(Into::into)
}

pub async fn update_binding_status<C: ConnectionTrait>(
    db: &C,
    binding: project_host_binding::Model,
    status: HostBindingStatus,
    failure_reason: Option<String>,
) -> anyhow::Result<project_host_binding::Model> {
    let mut active: project_host_binding::ActiveModel = binding.into();
    active.status = Set(status);
    active.failure_reason = Set(failure_reason);
    active.update(db).await.map_err(Into::into)
}

pub async fn soft_delete_binding<C: ConnectionTrait>(
    db: &C,
    binding: project_host_binding::Model,
) -> anyhow::Result<()> {
    soft_delete_binding_at(db, binding, OffsetDateTime::now_utc()).await
}

pub async fn soft_delete_binding_at<C: ConnectionTrait>(
    db: &C,
    binding: project_host_binding::Model,
    deleted_at: OffsetDateTime,
) -> anyhow::Result<()> {
    let mut active: project_host_binding::ActiveModel = binding.into();
    active.deleted_at = Set(Some(deleted_at));
    active.is_primary = Set(false);
    active.update(db).await?;
    Ok(())
}

/// Marks one binding as primary and clears the flag on every other binding
/// of the project. Callers should run this inside a transaction.
pub async fn set_primary_binding<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    binding_id: Uuid,
) -> anyhow::Result<()> {
    use sea_orm::sea_query::Expr;

    project_host_binding::Entity::update_many()
        .col_expr(project_host_binding::Column::IsPrimary, Expr::value(false))
        .filter(project_host_binding::Column::ProjectId.eq(project_id))
        .filter(project_host_binding::Column::IsPrimary.eq(true))
        .exec(db)
        .await?;
    project_host_binding::Entity::update_many()
        .col_expr(project_host_binding::Column::IsPrimary, Expr::value(true))
        .filter(project_host_binding::Column::Id.eq(binding_id))
        .exec(db)
        .await?;
    Ok(())
}

// --- Host policies ----------------------------------------------------------

pub async fn policy_for_team_group<C: ConnectionTrait>(
    db: &C,
    team_group_id: Option<Uuid>,
) -> anyhow::Result<Option<host_policy::Model>> {
    let Some(team_group_id) = team_group_id else {
        return Ok(None);
    };
    host_policy::Entity::find()
        .filter(host_policy::Column::TeamGroupId.eq(team_group_id))
        .one(db)
        .await
        .map_err(Into::into)
}

// --- Provision events -------------------------------------------------------

pub struct RecordProvisionEventParams {
    pub host_binding_id: Uuid,
    pub host_source_id: Option<Uuid>,
    pub status: HostProvisionEventStatus,
    pub operation: String,
    pub provider_request_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub metadata: serde_json::Value,
}

pub async fn record_provision_event<C: ConnectionTrait>(
    db: &C,
    params: RecordProvisionEventParams,
) -> anyhow::Result<host_provision_event::Model> {
    host_provision_event::ActiveModel {
        id: Set(Uuid::now_v7()),
        host_binding_id: Set(params.host_binding_id),
        host_source_id: Set(params.host_source_id),
        status: Set(params.status),
        operation: Set(params.operation),
        provider_request_id: Set(params.provider_request_id),
        error_code: Set(params.error_code),
        error_message: Set(params.error_message),
        metadata: Set(params.metadata),
        created_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(db)
    .await
    .map_err(Into::into)
}

pub async fn list_provision_events_for_binding<C: ConnectionTrait>(
    db: &C,
    binding_id: Uuid,
) -> anyhow::Result<Vec<host_provision_event::Model>> {
    host_provision_event::Entity::find()
        .filter(host_provision_event::Column::HostBindingId.eq(binding_id))
        .order_by_desc(host_provision_event::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

/// Builds the platform host for a project under a base domain. The first
/// attempt uses the bare project slug; later attempts append a random
/// four-character suffix to step around conflicts.
pub fn platform_host_candidate(project_slug: &str, base_domain: &str, attempt: u8) -> String {
    // A full label may be at most 63 characters; leave room for the suffix.
    let slug: String = project_slug.chars().take(58 - 5).collect();
    let slug = slug.trim_end_matches('-');
    if attempt == 0 {
        format!("{slug}.{base_domain}")
    } else {
        let suffix: String = {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            (0..4)
                .map(|_| {
                    let value = rng.gen_range(0..36u8);
                    char::from_digit(u32::from(value), 36).unwrap_or('0')
                })
                .collect()
        };
        format!("{slug}-{suffix}.{base_domain}")
    }
}

/// Builds the preview host for a deployment under a base domain, keeping a
/// single DNS label so one-level wildcard records still match.
///
/// The suffix comes from the END of the UUID: v7 ids start with a
/// timestamp, so their leading hex barely changes between deployments and
/// collides on the partial unique preview-host index. The tail bytes are
/// random.
pub fn preview_host_for(project_slug: &str, deployment_id: Uuid, base_domain: &str) -> String {
    let simple = deployment_id.simple().to_string();
    let short = &simple[simple.len() - 8..];
    let slug: String = project_slug.chars().take(63 - 9).collect();
    let slug = slug.trim_end_matches('-');
    format!("{slug}-{short}.{base_domain}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(
        enabled: bool,
        allows_auto_assign: bool,
        is_default: bool,
        label: &str,
    ) -> host_source::Model {
        host_source::Model {
            id: Uuid::now_v7(),
            kind: HostSourceKind::Wildcard,
            label: label.to_owned(),
            base_domain: "example.test".to_owned(),
            enabled,
            allows_auto_assign,
            is_default,
            provider: None,
            config: serde_json::json!({}),
            deleted_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn single_candidate_is_selected_directly() {
        let sources = vec![
            source(true, true, false, "one"),
            source(true, false, false, "no-auto"),
        ];
        match select_auto_assign_source(&sources) {
            AutoAssignSelection::Source(selected) => assert_eq!(selected.label, "one"),
            _ => panic!("expected the single auto-assign source"),
        }
    }

    #[test]
    fn multiple_candidates_require_exactly_one_default() {
        let no_default = vec![
            source(true, true, false, "a"),
            source(true, true, false, "b"),
        ];
        assert!(matches!(
            select_auto_assign_source(&no_default),
            AutoAssignSelection::NoDefault
        ));

        let one_default = vec![
            source(true, true, false, "a"),
            source(true, true, true, "b"),
        ];
        match select_auto_assign_source(&one_default) {
            AutoAssignSelection::Source(selected) => assert_eq!(selected.label, "b"),
            _ => panic!("expected the default source"),
        }
    }

    #[test]
    fn disabled_sources_are_never_selected() {
        let sources = vec![source(false, true, true, "disabled")];
        assert!(matches!(
            select_auto_assign_source(&sources),
            AutoAssignSelection::NoSource
        ));
    }

    #[test]
    fn platform_host_candidates_stay_within_label_rules() {
        assert_eq!(
            platform_host_candidate("demo", "grass.test", 0),
            "demo.grass.test"
        );
        let retry = platform_host_candidate("demo", "grass.test", 1);
        assert!(retry.starts_with("demo-"));
        assert!(retry.ends_with(".grass.test"));
        assert_eq!(retry.len(), "demo-xxxx.grass.test".len());
        assert!(grass_validator::normalize_host(&retry).is_ok());

        let long_slug = "a".repeat(80);
        let candidate = platform_host_candidate(&long_slug, "grass.test", 1);
        assert!(grass_validator::normalize_host(&candidate).is_ok());
    }

    #[test]
    fn preview_hosts_are_single_label_and_valid() {
        let deployment_id = Uuid::now_v7();
        let host = preview_host_for("my-app", deployment_id, "grass.test");
        assert!(grass_validator::normalize_host(&host).is_ok());
        assert_eq!(
            host.matches('.').count(),
            "grass.test".matches('.').count() + 1
        );

        let long = preview_host_for(&"b".repeat(90), deployment_id, "grass.test");
        assert!(grass_validator::normalize_host(&long).is_ok());
    }

    #[test]
    fn preview_hosts_differ_for_deployments_created_close_together() {
        // v7 uuids share their leading timestamp hex for ~18 hours; the
        // suffix must come from the random tail so back-to-back preview
        // deployments of one project never collide.
        let first = preview_host_for("my-app", Uuid::now_v7(), "grass.test");
        let second = preview_host_for("my-app", Uuid::now_v7(), "grass.test");
        assert_ne!(first, second);
    }

    #[test]
    fn domain_review_policy_defaults_to_auto_and_accepts_a_team_group_override() {
        assert_eq!(DomainReviewMode::default(), DomainReviewMode::Auto);
        assert_eq!(
            apply_domain_review_policy_override(
                DomainReviewMode::Auto,
                Some(&serde_json::json!({ "domain": "manual" })),
            ),
            DomainReviewMode::Manual
        );
        assert_eq!(
            apply_domain_review_policy_override(
                DomainReviewMode::Manual,
                Some(&serde_json::json!({ "production": "auto" })),
            ),
            DomainReviewMode::Manual
        );
    }

    #[tokio::test]
    async fn domain_review_policy_resolves_team_group_before_platform_default() {
        use crate::infra::database::entity::{
            SystemSettingValueKind, TeamKind, system_setting, team, team_group,
        };

        let group_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let now = OffsetDateTime::UNIX_EPOCH;
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![system_setting::Model {
                id: Uuid::now_v7(),
                key: "domain_review_policy.default".to_owned(),
                value_kind: SystemSettingValueKind::String,
                value: serde_json::json!("manual"),
                is_secret: false,
                created_at: now,
                updated_at: now,
            }]])
            .append_query_results([vec![team::Model {
                id: team_id,
                slug: "domain-review".to_owned(),
                name: "Domain Review".to_owned(),
                avatar_version: None,
                kind: TeamKind::Team,
                group_id: Some(group_id),
                explicit_quota_plan_id: None,
                owner_user_id: None,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            }]])
            .append_query_results([vec![team_group::Model {
                id: group_id,
                code: "domain-review".to_owned(),
                name: "Domain Review".to_owned(),
                description: None,
                quota_plan_id: None,
                review_policy: Some(serde_json::json!({ "domain": "auto" })),
                is_default: false,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            }]])
            .into_connection();

        assert_eq!(
            domain_review_policy_for_team(&db, team_id).await.unwrap(),
            DomainReviewMode::Auto
        );
    }

    #[tokio::test]
    async fn binding_mutations_lock_the_live_row() {
        let binding_id = Uuid::now_v7();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([Vec::<project_host_binding::Model>::new()])
            .into_connection();

        get_binding_by_id_for_update(&db, binding_id).await.unwrap();

        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("FOR UPDATE"), "{statements}");
    }

    #[tokio::test]
    async fn deletion_retries_can_lock_an_already_deleted_binding() {
        let binding_id = Uuid::now_v7();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([Vec::<project_host_binding::Model>::new()])
            .into_connection();

        get_binding_by_id_for_update_including_deleted(&db, binding_id)
            .await
            .unwrap();

        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("FOR UPDATE"), "{statements}");
        assert!(!statements.contains("deleted_at\" IS NULL"), "{statements}");
    }
}
