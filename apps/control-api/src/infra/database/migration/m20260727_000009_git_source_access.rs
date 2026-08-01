use grass_git_source::RepositoryUrlError;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;
use uuid::Uuid;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        connection
            .execute_unprepared(
                r#"CREATE TYPE source_credential_kind AS ENUM ('https', 'ssh');
CREATE TYPE ssh_host_key_status AS ENUM ('pending', 'approved', 'rejected', 'superseded');

CREATE TABLE source_credentials (
    id UUID PRIMARY KEY,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind source_credential_kind NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    username TEXT NULL,
    current_version_id UUID NULL,
    revoked_at TIMESTAMPTZ NULL,
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX uq_source_credentials_id_team ON source_credentials(id, team_id);
CREATE UNIQUE INDEX uq_source_credentials_active_name ON source_credentials(team_id, name) WHERE revoked_at IS NULL;
CREATE INDEX ix_source_credentials_team_endpoint ON source_credentials(team_id, kind, host, port);

CREATE TABLE source_credential_versions (
    id UUID PRIMARY KEY,
    credential_id UUID NOT NULL REFERENCES source_credentials(id) ON DELETE CASCADE,
    version INTEGER NOT NULL CHECK (version > 0),
    key_id TEXT NOT NULL,
    encrypted_payload JSONB NOT NULL,
    revoked_at TIMESTAMPTZ NULL,
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (credential_id, version)
);
ALTER TABLE source_credentials
    ADD CONSTRAINT fk_source_credentials_current_version
    FOREIGN KEY (current_version_id) REFERENCES source_credential_versions(id) ON DELETE RESTRICT;

CREATE UNIQUE INDEX uq_projects_id_team ON projects(id, team_id);
CREATE TABLE project_source_credentials (
    project_id UUID PRIMARY KEY,
    credential_id UUID NOT NULL,
    team_id UUID NOT NULL,
    bound_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_project_source_credentials_project_team
        FOREIGN KEY (project_id, team_id) REFERENCES projects(id, team_id) ON DELETE CASCADE,
    CONSTRAINT fk_project_source_credentials_credential_team
        FOREIGN KEY (credential_id, team_id) REFERENCES source_credentials(id, team_id) ON DELETE CASCADE
);
CREATE INDEX ix_project_source_credentials_credential ON project_source_credentials(credential_id);

CREATE TABLE ssh_host_keys (
    id UUID PRIMARY KEY,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    host TEXT NOT NULL,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    key_type TEXT NOT NULL,
    public_key TEXT NOT NULL,
    fingerprint_sha256 TEXT NOT NULL,
    status ssh_host_key_status NOT NULL DEFAULT 'pending',
    first_seen_node_id UUID NULL REFERENCES nodes(id) ON DELETE SET NULL,
    approved_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    approved_at TIMESTAMPTZ NULL,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (team_id, host, port, fingerprint_sha256)
);
CREATE UNIQUE INDEX uq_ssh_host_keys_approved_endpoint ON ssh_host_keys(team_id, host, port) WHERE status = 'approved';
CREATE INDEX ix_ssh_host_keys_pending ON ssh_host_keys(team_id, status, created_at) WHERE status = 'pending';

CREATE TABLE source_credential_leases (
    id UUID PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE CHECK (char_length(token_hash) = 64),
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    deployment_id UUID NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    credential_version_id UUID NOT NULL REFERENCES source_credential_versions(id) ON DELETE RESTRICT,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (deployment_id)
);
CREATE INDEX ix_source_credential_leases_expiry ON source_credential_leases(expires_at) WHERE consumed_at IS NULL;

ALTER TABLE deployments ADD COLUMN source_credential_version_id UUID NULL;
ALTER TABLE deployments
    ADD CONSTRAINT fk_deployments_source_credential_version
    FOREIGN KEY (source_credential_version_id) REFERENCES source_credential_versions(id) ON DELETE RESTRICT;"#,
            )
            .await?;

        clear_unsafe_repository_urls(connection).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"UPDATE projects SET source_config = source_config - 'repository_migration' WHERE source_config ? 'repository_migration';
ALTER TABLE deployments DROP CONSTRAINT IF EXISTS fk_deployments_source_credential_version;
ALTER TABLE deployments DROP COLUMN IF EXISTS source_credential_version_id;
DROP TABLE IF EXISTS source_credential_leases;
DROP TABLE IF EXISTS ssh_host_keys;
DROP TABLE IF EXISTS project_source_credentials;
DROP INDEX IF EXISTS uq_projects_id_team;
ALTER TABLE source_credentials DROP CONSTRAINT IF EXISTS fk_source_credentials_current_version;
DROP TABLE IF EXISTS source_credential_versions;
DROP TABLE IF EXISTS source_credentials;
DROP TYPE IF EXISTS ssh_host_key_status;
DROP TYPE IF EXISTS source_credential_kind;"#,
            )
            .await?;
        Ok(())
    }
}

async fn clear_unsafe_repository_urls<C: ConnectionTrait>(connection: &C) -> Result<(), DbErr> {
    let rows = connection
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, repository_url FROM projects WHERE repository_url IS NOT NULL".to_owned(),
        ))
        .await?;

    for row in rows {
        let id: Uuid = row.try_get("", "id")?;
        let repository_url: String = row.try_get("", "repository_url")?;
        let Some(reason) = migration_rejection_reason(&repository_url) else {
            continue;
        };
        connection
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"UPDATE projects
SET repository_url = NULL,
    source_config = COALESCE(source_config, '{}'::jsonb) || jsonb_build_object(
        'repository_migration',
        jsonb_build_object('status', 'cleared', 'reason', $2::text, 'migrated_at', '2026-07-27T00:00:00Z')
    )
WHERE id = $1 AND repository_url IS NOT NULL"#,
                [id.into(), reason.into()],
            ))
            .await?;
    }
    Ok(())
}

fn migration_rejection_reason(value: &str) -> Option<&'static str> {
    grass_git_source::parse_repository_url(value)
        .err()
        .map(|error| match error {
            RepositoryUrlError::Invalid => "invalid_url",
            RepositoryUrlError::UnsupportedTransport => "unsupported_transport",
            RepositoryUrlError::EmbeddedCredential => "embedded_credential",
        })
}

#[cfg(test)]
mod tests {
    use super::migration_rejection_reason;

    #[test]
    fn repository_migration_preserves_supported_urls() {
        for value in [
            "http://example.com/repo.git",
            "https://example.com/repo.git",
            "ssh://git@example.com:2222/repo.git",
            "git@example.com:repo.git",
            "git://example.com/repo.git",
        ] {
            assert_eq!(migration_rejection_reason(value), None, "cleared {value}");
        }
    }

    #[test]
    fn repository_migration_records_only_safe_reason_codes() {
        assert_eq!(migration_rejection_reason("../repo"), Some("invalid_url"));
        assert_eq!(
            migration_rejection_reason("file:///srv/private/repo"),
            Some("unsupported_transport")
        );
        assert_eq!(
            migration_rejection_reason("https://token@example.com/repo.git"),
            Some("embedded_credential")
        );
    }
}
