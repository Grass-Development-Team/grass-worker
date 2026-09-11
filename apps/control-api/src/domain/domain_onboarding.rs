//! Persistent connection checks, independent from browsers and ACME order retries.
use super::{
    domain_dns::{ConnectionState, Resolver},
    ingress::{self, DnsVerification},
};
use crate::infra::database::entity::{
    HostBindingKind, HostBindingStatus, HostReviewStatus, domain_onboarding as check,
    project_host_binding as binding, regional_ingress, user,
};
use anyhow::Context;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbBackend,
    EntityTrait, QueryFilter, QuerySelect, Set, Statement, TransactionTrait, sea_query::Expr,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub async fn create<C: ConnectionTrait>(
    db: &C,
    binding: &binding::Model,
    actor: Uuid,
) -> anyhow::Result<()> {
    let user = user::Entity::find_by_id(actor)
        .one(db)
        .await?
        .context("Domain owner account was not found")?;
    check::ActiveModel {
        binding_id: Set(binding.id),
        created_by_user_id: Set(Some(actor)),
        contact_email: Set(user.email),
        dns_status: Set("pending".to_owned()),
        dns_error: Set(None),
        checked_at: Set(None),
        next_check_at: Set(OffsetDateTime::now_utc()),
        lease_until: Set(None),
    }
    .insert(db)
    .await?;
    Ok(())
}
pub async fn get<C: ConnectionTrait>(db: &C, id: Uuid) -> anyhow::Result<Option<check::Model>> {
    Ok(check::Entity::find_by_id(id).one(db).await?)
}
pub fn next_delay(id: Uuid, now: OffsetDateTime) -> Duration {
    Duration::seconds(60 + ((id.as_u128() as u64 ^ now.unix_timestamp() as u64) % 61) as i64)
}
pub fn ready_for_issuance(check: &check::Model, now: OffsetDateTime) -> bool {
    check.dns_status == "ready"
        && !check.contact_email.trim().is_empty()
        && check
            .checked_at
            .is_some_and(|at| now - at <= Duration::minutes(5))
}
fn apply_ownership(
    binding: &binding::Model,
    state: ConnectionState,
    ownership: DnsVerification,
    now: OffsetDateTime,
) -> binding::ActiveModel {
    let mut active: binding::ActiveModel = binding.clone().into();
    active.ownership_checked_at = Set(Some(now));
    match ownership {
        DnsVerification::Verified => {
            active.ownership_status = Set("verified".to_owned());
            active.ownership_error = Set(None);
            if state == ConnectionState::Ready
                && matches!(
                    binding.review_status,
                    HostReviewStatus::Approved | HostReviewStatus::NotRequired
                )
                && !matches!(binding.status, HostBindingStatus::Disabled)
            {
                active.status = Set(HostBindingStatus::Active);
            }
        }
        DnsVerification::Missing | DnsVerification::Mismatch => {
            active.ownership_status = Set("failed".to_owned());
            active.ownership_error = Set(Some(
                if ownership == DnsVerification::Missing {
                    "Add the displayed TXT ownership record. It will be checked automatically."
                } else {
                    "The TXT ownership record does not match this domain binding."
                }
                .to_owned(),
            ));
            if !matches!(binding.status, HostBindingStatus::Disabled) {
                active.status = Set(HostBindingStatus::Pending);
            }
        }
    }
    active.updated_at = Set(now);
    active
}

pub async fn run_check(
    db: &DatabaseConnection,
    id: Uuid,
    secret: &str,
    force: bool,
) -> anyhow::Result<()> {
    run_check_with_resolver(db, id, secret, force, &Resolver::new()?).await
}
pub(crate) async fn run_check_with_resolver(
    db: &DatabaseConnection,
    id: Uuid,
    secret: &str,
    force: bool,
    resolver: &Resolver,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let lease = now + Duration::seconds(45);
    let mut claim = check::Entity::update_many()
        .col_expr(check::Column::LeaseUntil, Expr::value(lease))
        .filter(check::Column::BindingId.eq(id))
        .filter(
            Condition::any()
                .add(check::Column::LeaseUntil.is_null())
                .add(check::Column::LeaseUntil.lte(now)),
        );
    if !force {
        claim = claim.filter(check::Column::NextCheckAt.lte(now));
    }
    if claim.exec(db).await?.rows_affected == 0 {
        return Ok(());
    }
    let Some(binding) = binding::Entity::find_by_id(id)
        .filter(binding::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Ok(());
    };
    if binding.kind != HostBindingKind::Custom || binding.status == HostBindingStatus::Disabled {
        return Ok(());
    }
    let entry = ingress::get_enabled_by_region(db, &binding.region).await?;
    let result = match &entry {
        Some(entry) => {
            let expected = ingress::dns_verification_token(secret, id, &binding.host);
            match tokio::time::timeout(std::time::Duration::from_secs(15), async {
                tokio::try_join!(
                    resolver.connection(&binding.host, &entry.hostname),
                    resolver.ownership(&binding.host, &expected)
                )
            })
            .await
            {
                Ok(result) => result.map(Some),
                Err(_) => Err(anyhow::anyhow!("Public DNS check timed out")),
            }
        }
        None => Ok(None),
    };
    // Do not apply a result for an entry edited or disabled while DNS was queried.
    let transaction = db.begin().await?;
    let current_entry = if let Some(entry) = &entry {
        regional_ingress::Entity::find_by_id(entry.id)
            .lock_shared()
            .one(&transaction)
            .await?
    } else {
        None
    };
    let entry_unchanged =
        current_entry
            .as_ref()
            .zip(entry.as_ref())
            .is_some_and(|(current, original)| {
                current.enabled
                    && current.deleted_at.is_none()
                    && current.hostname == original.hostname
            });
    let current = super::hosts::get_binding_by_id_for_update(&transaction, id).await?;
    let Some(current) = current else {
        return Ok(());
    };
    let current_check = check::Entity::find_by_id(id)
        .filter(check::Column::LeaseUntil.eq(lease))
        .lock_exclusive()
        .one(&transaction)
        .await?;
    if current_check.is_none() {
        return Ok(());
    }
    let now = OffsetDateTime::now_utc();
    let (status, error) = if current.status == HostBindingStatus::Disabled {
        ("pending", None)
    } else if !entry_unchanged {
        (
            "entry_unavailable",
            Some("This region has no enabled entry. Contact a platform administrator.".to_owned()),
        )
    } else {
        match result {
            Ok(Some((state, ownership))) => {
                apply_ownership(&current, state, ownership, now)
                    .update(&transaction)
                    .await?;
                (state.status(), state.message().map(str::to_owned))
            }
            Ok(None) => (
                "entry_unavailable",
                Some("This region has no enabled entry.".to_owned()),
            ),
            Err(_) => (
                "error",
                Some(
                    "Public DNS could not be checked. The server will retry automatically."
                        .to_owned(),
                ),
            ),
        }
    };
    check::Entity::update_many()
        .col_expr(check::Column::DnsStatus, Expr::value(status))
        .col_expr(check::Column::DnsError, Expr::value(error))
        .col_expr(check::Column::CheckedAt, Expr::value(now))
        .col_expr(
            check::Column::NextCheckAt,
            Expr::value(now + next_delay(id, now)),
        )
        .col_expr(
            check::Column::LeaseUntil,
            Expr::value(Option::<OffsetDateTime>::None),
        )
        .filter(check::Column::BindingId.eq(id))
        .filter(check::Column::LeaseUntil.eq(lease))
        .exec(&transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn sweep(db: &DatabaseConnection, secret: &str) -> anyhow::Result<()> {
    let due = db
        .query_all_raw(Statement::from_string(
            DbBackend::Postgres,
            r#"
        SELECT d.binding_id
        FROM domain_onboarding d
        JOIN project_host_bindings b ON b.id = d.binding_id
        WHERE b.deleted_at IS NULL AND b.status <> 'disabled'
          AND d.next_check_at <= CURRENT_TIMESTAMP
          AND (d.lease_until IS NULL OR d.lease_until <= CURRENT_TIMESTAMP)
        ORDER BY d.next_check_at
        LIMIT 64
    "#,
        ))
        .await?;
    let mut tasks = tokio::task::JoinSet::new();
    for row in due {
        let id: Uuid = row.try_get("", "binding_id")?;
        let db = db.clone();
        let secret = secret.to_owned();
        tasks.spawn(async move {
            if run_check(&db, id, &secret, false).await.is_err() {
                tracing::warn!(
                    operation = "control_api.domain_connection_check",
                    binding_id = %id,
                    "Domain connection check failed; its lease will expire for retry"
                );
            }
        });
        if tasks.len() >= 8 {
            let _ = tasks.join_next().await;
        }
    }
    while tasks.join_next().await.is_some() {}
    Ok(())
}

pub fn view(check: &check::Model) -> serde_json::Value {
    serde_json::json!({
        "dns_status": check.dns_status,
        "dns_error": check.dns_error,
        "checked_at": crate::infra::http::timestamps::ts(check.checked_at),
        "next_check_at": crate::infra::http::timestamps::ts(Some(check.next_check_at)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn check_interval_is_between_one_and_two_minutes_and_uses_jitter() {
        let id = Uuid::now_v7();
        let now = OffsetDateTime::now_utc();
        let delays = (0..120)
            .map(|i| next_delay(id, now + Duration::seconds(i)).whole_seconds())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(delays.len() > 1);
        assert!(delays.iter().all(|d| (60..=120).contains(d)));
    }
    #[test]
    fn dns_success_never_bypasses_ownership_review_or_disablement() {
        let mut binding = super::super::certificates::tests::binding_fixture();
        binding.status = HostBindingStatus::Pending;
        let now = OffsetDateTime::now_utc();
        assert_eq!(
            apply_ownership(
                &binding,
                ConnectionState::Ready,
                DnsVerification::Verified,
                now
            )
            .status,
            Set(HostBindingStatus::Active)
        );
        assert_eq!(
            apply_ownership(
                &binding,
                ConnectionState::Ready,
                DnsVerification::Mismatch,
                now
            )
            .status,
            Set(HostBindingStatus::Pending)
        );
        binding.review_status = HostReviewStatus::Pending;
        assert_ne!(
            apply_ownership(
                &binding,
                ConnectionState::Ready,
                DnsVerification::Verified,
                now
            )
            .status,
            Set(HostBindingStatus::Active)
        );
        binding.review_status = HostReviewStatus::Approved;
        binding.status = HostBindingStatus::Disabled;
        assert_ne!(
            apply_ownership(
                &binding,
                ConnectionState::Ready,
                DnsVerification::Verified,
                now
            )
            .status,
            Set(HostBindingStatus::Active)
        );
        binding.status = HostBindingStatus::Pending;
        assert_ne!(
            apply_ownership(
                &binding,
                ConnectionState::Mismatch,
                DnsVerification::Verified,
                now
            )
            .status,
            Set(HostBindingStatus::Active)
        );
    }
    #[test]
    fn issuing_requires_a_recent_successful_check_and_the_users_contact_email() {
        let now = OffsetDateTime::now_utc();
        let mut check = check::Model {
            binding_id: Uuid::now_v7(),
            created_by_user_id: None,
            contact_email: "owner@example.com".into(),
            dns_status: "ready".into(),
            dns_error: None,
            checked_at: Some(now),
            next_check_at: now,
            lease_until: None,
        };
        assert!(ready_for_issuance(&check, now));
        check.dns_status = "mismatch".into();
        assert!(!ready_for_issuance(&check, now));
        check.dns_status = "ready".into();
        check.checked_at = Some(now - Duration::minutes(6));
        assert!(!ready_for_issuance(&check, now));
        check.checked_at = Some(now);
        check.contact_email.clear();
        assert!(!ready_for_issuance(&check, now));
    }
}
