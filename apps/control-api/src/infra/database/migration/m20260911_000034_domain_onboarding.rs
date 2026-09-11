use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
DELETE FROM managed_certificates WHERE host_binding_id IS NULL;
DROP INDEX ux_managed_certificates_regional;
ALTER TABLE managed_certificates
    ALTER COLUMN host_binding_id SET NOT NULL,
    ADD COLUMN contact_email TEXT NOT NULL DEFAULT '',
    DROP COLUMN dns_record_name, DROP COLUMN dns_record_value, DROP COLUMN dns_cleanup;
UPDATE managed_certificates SET challenge_method = 'http01', challenge_token = NULL, challenge_value = NULL, challenge_expires_at = NULL, lease_until = NULL;
ALTER TABLE managed_certificates DROP CONSTRAINT managed_certificates_challenge_method_check;
ALTER TABLE managed_certificates ADD CONSTRAINT managed_certificates_challenge_method_check CHECK (challenge_method = 'http01');
ALTER TABLE regional_ingresses
    DROP COLUMN tls_enabled, DROP COLUMN certificate_issuer, DROP COLUMN certificate_auto_renew,
    DROP COLUMN certificate_status, DROP COLUMN certificate_expires_at, DROP COLUMN certificate_error,
    DROP COLUMN dns_challenge_provider, DROP COLUMN dns_challenge_config, DROP COLUMN dns_challenge_status,
    DROP COLUMN dns_challenge_record_name, DROP COLUMN dns_challenge_record_value,
    DROP COLUMN acme_account, DROP COLUMN certificate_bundle, DROP COLUMN certificate_issued_at,
    ADD COLUMN dns_status TEXT NOT NULL DEFAULT 'pending' CHECK (dns_status IN ('pending','resolved','unresolved','error')),
    ADD COLUMN dns_checked_at TIMESTAMPTZ NULL,
    ADD COLUMN dns_error TEXT NULL;
CREATE TABLE domain_onboarding (
    binding_id UUID PRIMARY KEY REFERENCES project_host_bindings(id) ON DELETE CASCADE,
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    contact_email TEXT NOT NULL,
    dns_status TEXT NOT NULL DEFAULT 'pending' CHECK (dns_status IN ('pending','ready','unresolved','mismatch','entry_unavailable','error')),
    dns_error TEXT NULL,
    checked_at TIMESTAMPTZ NULL,
    next_check_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_until TIMESTAMPTZ NULL
);
CREATE INDEX ix_domain_onboarding_due ON domain_onboarding(next_check_at, lease_until);
INSERT INTO domain_onboarding (binding_id, created_by_user_id, contact_email)
SELECT b.id, u.id, u.email FROM project_host_bindings b
JOIN projects p ON p.id = b.project_id JOIN users u ON u.id = p.created_by_user_id
WHERE b.kind = 'custom';
UPDATE managed_certificates c SET contact_email = d.contact_email FROM domain_onboarding d WHERE d.binding_id = c.host_binding_id;
"#;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(r#"
DROP TABLE domain_onboarding;
ALTER TABLE regional_ingresses DROP COLUMN dns_status, DROP COLUMN dns_checked_at, DROP COLUMN dns_error,
    ADD COLUMN tls_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN certificate_issuer TEXT NOT NULL DEFAULT 'letsencrypt' CONSTRAINT ck_regional_ingresses_certificate_issuer CHECK (certificate_issuer IN ('letsencrypt','zerossl','manual')),
    ADD COLUMN certificate_auto_renew BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN certificate_status TEXT NOT NULL DEFAULT 'pending' CONSTRAINT ck_regional_ingresses_certificate_status CHECK (certificate_status IN ('pending','issuing','active','expiring','failed','disabled')),
    ADD COLUMN certificate_expires_at TIMESTAMPTZ NULL, ADD COLUMN certificate_error TEXT NULL,
    ADD COLUMN dns_challenge_provider TEXT NULL, ADD COLUMN dns_challenge_config JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN dns_challenge_status TEXT NOT NULL DEFAULT 'not_configured' CONSTRAINT ck_regional_ingresses_dns_challenge_status CHECK (dns_challenge_status IN ('not_configured','pending','valid','failed')),
    ADD COLUMN dns_challenge_record_name TEXT NULL, ADD COLUMN dns_challenge_record_value TEXT NULL,
    ADD COLUMN acme_account JSONB NULL, ADD COLUMN certificate_bundle JSONB NULL, ADD COLUMN certificate_issued_at TIMESTAMPTZ NULL;
ALTER TABLE managed_certificates ALTER COLUMN host_binding_id DROP NOT NULL, DROP COLUMN contact_email,
    ADD COLUMN dns_record_name TEXT NULL, ADD COLUMN dns_record_value TEXT NULL, ADD COLUMN dns_cleanup JSONB NULL,
    DROP CONSTRAINT managed_certificates_challenge_method_check;
ALTER TABLE managed_certificates ADD CONSTRAINT managed_certificates_challenge_method_check CHECK (challenge_method IN ('http01','dns01'));
CREATE UNIQUE INDEX ux_managed_certificates_regional ON managed_certificates(ingress_id) WHERE host_binding_id IS NULL;
"#).await?;
        Ok(())
    }
}
