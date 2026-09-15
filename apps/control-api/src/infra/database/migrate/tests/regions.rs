use anyhow::ensure;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::MigratorTrait;
use uuid::Uuid;

use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::support::{
    PostgresMigrationDatabase, assert_migration_tracking, object_count, query_column_shapes,
};

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_region_catalog_backfills_and_enforces_references() -> anyhow::Result<()> {
    let _guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")?;
    let database = PostgresMigrationDatabase::start(&database_url).await?;
    let result: anyhow::Result<()> = async {
        Migrator::up(&database.db, Some(32)).await?;
        database
            .db
            .execute_unprepared(
                "INSERT INTO regional_ingresses (id, region, hostname, created_at, \
                    updated_at) VALUES ('00000000-0000-0000-0000-000000000101', 'hk_1', \
                    'hk.entry.example.com', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .await?;
        Migrator::up(&database.db, Some(1)).await?;
        let rows = database
            .db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT code FROM regions ORDER BY code".to_owned(),
            ))
            .await?;
        let codes = rows
            .iter()
            .map(|r| r.try_get::<String>("", "code"))
            .collect::<Result<Vec<_>, _>>()?;
        ensure!(codes == vec!["default", "hk_1"]);
        let foreign_keys = object_count(
            &database.db,
            "SELECT count(*) AS count FROM pg_constraint WHERE contype = 'f' AND \
                    confrelid = 'regions'::regclass",
        )
        .await?;
        ensure!(foreign_keys == 5);
        ensure!(
            database
                .db
                .execute_unprepared("DELETE FROM regions WHERE code = 'hk_1'")
                .await
                .is_err()
        );
        ensure!(
            database
                .db
                .execute_unprepared("INSERT INTO regions (code, name) VALUES ('hk_1', 'duplicate')")
                .await
                .is_err()
        );
        ensure!(
            database
                .db
                .execute_unprepared(
                    "INSERT INTO regional_ingresses (id, region, hostname, created_at, \
                    updated_at) VALUES ('00000000-0000-0000-0000-000000000102', 'unknown', \
                    'unknown.entry.example.com', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
                )
                .await
                .is_err()
        );
        ensure!(
            database
                .db
                .execute_unprepared(
                    "INSERT INTO regional_ingresses (id, region, hostname, created_at, \
                    updated_at) VALUES ('00000000-0000-0000-0000-000000000103', 'hk_1', \
                    'second.entry.example.com', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
                )
                .await
                .is_err()
        );
        let columns = query_column_shapes(
            &database.db,
            "SELECT column_name, udt_name, is_nullable, column_default FROM \
                    information_schema.columns WHERE table_schema = current_schema() AND \
                    table_name = 'regions'",
        )
        .await?;
        ensure!(
            columns
                .iter()
                .any(|c| c.name == "code" && c.nullable == "NO" && c.udt_name == "text")
        );
        ensure!(
            columns
                .iter()
                .any(|c| c.name == "name" && c.nullable == "NO")
        );
        Ok(())
    }
    .await;
    database.cleanup().await?;
    result
}

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_domain_onboarding_migrates_and_checks_customer_dns() -> anyhow::Result<()> {
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};
    use serde_json::json;

    use crate::domain::{acme, certificate_settings, certificates, domain_onboarding};
    use crate::infra::database::entity::{
        HostBindingStatus, managed_certificate, project_host_binding as binding, regional_ingress,
    };

    let _guard = MIGRATION_TEST_LOCK.lock().await;
    let database =
        PostgresMigrationDatabase::start(&std::env::var("GRASS_TEST_DATABASE_URL")?).await?;
    let result: anyhow::Result<()> = async {
        let db = &database.db;
        Migrator::up(db, Some(33)).await?;
        let owner = Uuid::now_v7();
        let actor = Uuid::now_v7();
        let mut old = crate::test_support::certificates::binding_fixture();
        old.host = "legacy.example.org".into();
        let entry_id = Uuid::now_v7();
        db.execute_unprepared(&format!(r#"
                INSERT INTO users (id, email, display_name) VALUES
                    ('{owner}', 'owner@example.org', 'Owner'),
                    ('{actor}', 'adder@example.org', 'Adding user');
                INSERT INTO teams (id, slug, name, owner_user_id)
                VALUES ('{}', 'onboarding', 'Onboarding', '{owner}');
                INSERT INTO projects (id, team_id, slug, name, created_by_user_id)
                VALUES ('{}', '{}', 'onboarding', 'Onboarding', '{owner}');
                INSERT INTO regions (code, name) VALUES ('eu', 'Europe');
                INSERT INTO regional_ingresses (id, region, hostname, created_at, updated_at)
                VALUES ('{entry_id}', 'eu', 'entry.example.org', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);
            "#, old.team_id, old.project_id, old.team_id)).await?;
        binding::ActiveModel::from(old.clone()).insert(db).await?;
        db.execute_unprepared(&format!(r#"
                INSERT INTO managed_certificates (id, ingress_id, hostname, issuer, generation)
                VALUES ('{entry_id}', '{entry_id}', 'entry.example.org', 'letsencrypt', '{entry_id}');
                INSERT INTO managed_certificates (
                    id, ingress_id, host_binding_id, hostname, issuer, generation, challenge_method
                ) VALUES (
                    '{0}', '{entry_id}', '{0}', 'legacy.example.org', 'letsencrypt', '{0}', 'dns01'
                );
            "#, old.id)).await?;
        Migrator::up(db, None).await?;
        assert_migration_tracking(db, 35).await?;
        ensure!(
            managed_certificate::Entity::find_by_id(entry_id)
                .one(db)
                .await?
                .is_none(),
            "entry certificate must be removed"
        );
        let legacy = managed_certificate::Entity::find_by_id(old.id)
            .one(db)
            .await?
            .unwrap();
        ensure!(legacy.challenge_method == "http01" && legacy.contact_email == "owner@example.org");
        let entry_columns = query_column_shapes(
            db,
            "SELECT column_name, udt_name, is_nullable, column_default FROM \
                    information_schema.columns WHERE table_schema = current_schema() AND \
                    table_name = 'regional_ingresses'",
        )
        .await?;
        ensure!(
            !entry_columns
                .iter()
                .any(|c| c.name.starts_with("certificate_")
                    || c.name.starts_with("dns_challenge")
                    || c.name == "tls_enabled"
                    || c.name == "acme_account")
        );
        ensure!(entry_columns.iter().any(|c| c.name == "dns_checked_at"
            && c.udt_name == "timestamptz"
            && c.nullable == "YES"
            && c.default.is_none()));
        let columns = query_column_shapes(
            db,
            "SELECT column_name, udt_name, is_nullable, column_default FROM \
                    information_schema.columns WHERE table_schema = current_schema() AND \
                    table_name = 'domain_onboarding'",
        )
        .await?;
        for name in ["checked_at", "lease_until"] {
            ensure!(columns.iter().any(|c| c.name == name
                && c.udt_name == "timestamptz"
                && c.nullable == "YES"
                && c.default.is_none()));
        }
        ensure!(
            columns
                .iter()
                .any(|c| c.name == "next_check_at" && c.nullable == "NO" && c.default.is_some())
        );
        ensure!(
            columns
                .iter()
                .any(|c| c.name == "contact_email" && c.udt_name == "text" && c.nullable == "NO")
        );
        ensure!(
            object_count(
                db,
                "SELECT count(*) AS count FROM pg_constraint WHERE conrelid = \
                    'domain_onboarding'::regclass AND contype = 'f'"
            )
            .await?
                == 2
        );
        ensure!(
            object_count(
                db,
                "SELECT count(*) AS count FROM pg_indexes WHERE schemaname = \
                    current_schema() AND indexname = 'ix_domain_onboarding_due'"
            )
            .await?
                == 1
        );
        ensure!(
            db.execute_unprepared("UPDATE managed_certificates SET host_binding_id = NULL")
                .await
                .is_err()
        );
        ensure!(
            db.execute_unprepared("UPDATE managed_certificates SET challenge_method = 'dns01'")
                .await
                .is_err()
        );
        ensure!(
            db.execute_unprepared("UPDATE domain_onboarding SET dns_status = 'invalid'")
                .await
                .is_err()
        );
        let mut custom = old.clone();
        custom.id = Uuid::now_v7();
        custom.host = "site.example.org".into();
        custom.status = HostBindingStatus::Pending;
        custom.ownership_status = "pending".into();
        binding::ActiveModel::from(custom.clone())
            .insert(db)
            .await?;
        domain_onboarding::create(db, &custom, actor).await?;
        let contact = domain_onboarding::get(db, custom.id).await?.unwrap();
        ensure!(
            contact.created_by_user_id == Some(actor)
                && contact.contact_email == "adder@example.org"
        );

        let entry = regional_ingress::Entity::find_by_id(entry_id)
            .one(db)
            .await?
            .unwrap();
        let token =
            crate::domain::ingress::dns_verification_token("secret", custom.id, &custom.host);
        for (address, txt, expected, active) in [
            (None, "wrong", "unresolved", false),
            (Some("203.0.113.9"), token.as_str(), "mismatch", false),
            (Some("203.0.113.1"), "wrong", "ready", false),
            (Some("203.0.113.1"), token.as_str(), "ready", true),
        ] {
            let mut records = vec![
                (
                    "entry.example.org",
                    "A",
                    crate::test_support::dns::answer("entry.example.org", 1, "203.0.113.1"),
                ),
                (
                    "_grass.site.example.org",
                    "TXT",
                    crate::test_support::dns::answer(
                        "_grass.site.example.org",
                        16,
                        &format!("\"{txt}\""),
                    ),
                ),
            ];
            if let Some(address) = address {
                records.push((
                    "site.example.org",
                    "A",
                    crate::test_support::dns::answer("site.example.org", 1, address),
                ));
            }
            let (resolver, server) = crate::test_support::dns::fixture(records).await;
            // Immediate checks and scheduled checks use the same persisted workflow.
            domain_onboarding::run_check_with_resolver(db, custom.id, "secret", true, &resolver)
                .await?;
            server.abort();
            let check = domain_onboarding::get(db, custom.id).await?.unwrap();
            let bound = binding::Entity::find_by_id(custom.id)
                .one(db)
                .await?
                .unwrap();
            ensure!(
                check.dns_status == expected,
                "unexpected DNS state: {}",
                check.dns_status
            );
            ensure!((bound.status == HostBindingStatus::Active) == active);
            ensure!(check.lease_until.is_none());
            let interval = check.next_check_at - check.checked_at.unwrap();
            ensure!((60..=120).contains(&interval.whole_seconds()));
        }
        // The scheduler creates only customer-domain certificates, even without a deployment.
        acme::sweep(db, "secret").await?;
        let cert = managed_certificate::Entity::find_by_id(custom.id)
            .one(db)
            .await?
            .unwrap();
        ensure!(
            cert.hostname == custom.host
                && cert.contact_email == "adder@example.org"
                && cert.auto_renew
                && cert.challenge_method == "http01"
        );
        ensure!(
            cert.status == "pending",
            "no eligible entry nodes means no external order"
        );
        ensure!(
            managed_certificate::Entity::find_by_id(entry_id)
                .one(db)
                .await?
                .is_none()
        );
        let again = certificates::ensure_record(db, &entry, Some(&custom)).await?;
        ensure!(again.id == cert.id && again.generation == cert.generation);
        // Not-yet-due checks do not query DNS; leases also protect forced checks.
        let (resolver, server) = crate::test_support::dns::fixture(vec![]).await;
        let before = domain_onboarding::get(db, custom.id).await?.unwrap();
        domain_onboarding::run_check_with_resolver(db, custom.id, "secret", false, &resolver)
            .await?;
        ensure!(domain_onboarding::get(db, custom.id).await?.unwrap() == before);
        db.execute_unprepared(&format!(
            "UPDATE domain_onboarding SET lease_until = CURRENT_TIMESTAMP + INTERVAL '1 \
                    minute' WHERE binding_id = '{}'",
            custom.id
        ))
        .await?;
        domain_onboarding::run_check_with_resolver(db, custom.id, "secret", true, &resolver)
            .await?;
        ensure!(
            domain_onboarding::get(db, custom.id)
                .await?
                .unwrap()
                .dns_status
                == "ready"
        );
        // Simulate a restart after an abandoned lease. The next scheduled check recovers.
        db.execute_unprepared(&format!(
            "UPDATE domain_onboarding SET lease_until = CURRENT_TIMESTAMP - INTERVAL '1 \
                    second', next_check_at = CURRENT_TIMESTAMP - INTERVAL '1 second' WHERE \
                    binding_id = '{}'",
            custom.id
        ))
        .await?;
        domain_onboarding::run_check_with_resolver(db, custom.id, "secret", false, &resolver)
            .await?;
        ensure!(
            domain_onboarding::get(db, custom.id)
                .await?
                .unwrap()
                .dns_status
                == "entry_unavailable"
        );
        server.abort();
        let settings = certificate_settings::CertificateSettings {
            issuer: "zerossl".into(),
            eab: json!({
                "eab_kid": "test-id",
                "eab_hmac_key": "c2VjcmV0",
            }),
        };
        certificate_settings::save(db, &settings, "secret").await?;
        ensure!(certificate_settings::issuer(db).await? == "zerossl");
        let loaded = certificate_settings::load(db, "secret").await?;
        ensure!(loaded.eab == settings.eab);
        let stored = crate::domain::settings::get_setting(db, "domain_https")
            .await?
            .unwrap();
        ensure!(!stored.value.to_string().contains("c2VjcmV0"));
        let mut disabled: binding::ActiveModel = binding::Entity::find_by_id(custom.id)
            .one(db)
            .await?
            .unwrap()
            .into();
        disabled.status = Set(HostBindingStatus::Disabled);
        disabled.update(db).await?;
        let (resolver, server) = crate::test_support::dns::fixture(vec![]).await;
        domain_onboarding::run_check_with_resolver(db, custom.id, "secret", true, &resolver)
            .await?;
        ensure!(
            binding::Entity::find_by_id(custom.id)
                .one(db)
                .await?
                .unwrap()
                .status
                == HostBindingStatus::Disabled
        );
        server.abort();
        // Down/up restores the legacy shape, while reapplication produces the same new constraints.
        Migrator::down(db, Some(2)).await?;
        assert_migration_tracking(db, 33).await?;
        Migrator::up(db, None).await?;
        assert_migration_tracking(db, 35).await?;
        Ok(())
    }.await;
    database.cleanup().await?;
    result
}
