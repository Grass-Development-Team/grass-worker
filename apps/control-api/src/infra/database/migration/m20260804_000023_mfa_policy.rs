use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TABLE user_mfa_policies (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    inherit_platform BOOLEAN NOT NULL DEFAULT true,
    minimum_factors SMALLINT NOT NULL DEFAULT 0,
    required_factors JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_user_mfa_policies_minimum_factors CHECK (minimum_factors >= 0 AND minimum_factors <= 2),
    CONSTRAINT ck_user_mfa_policies_required_factors CHECK (jsonb_typeof(required_factors) = 'array')
);

INSERT INTO user_mfa_policies (user_id, inherit_platform, minimum_factors, required_factors, created_at, updated_at)
SELECT users.id, false, 1, '[]'::jsonb, NOW(), NOW()
FROM system_settings AS setting
CROSS JOIN LATERAL jsonb_array_elements_text(
    CASE
        WHEN jsonb_typeof(setting.value -> 'selected_user_ids') = 'array'
            THEN setting.value -> 'selected_user_ids'
        ELSE '[]'::jsonb
    END
) AS selected(user_id)
JOIN users ON users.id::text = selected.user_id
WHERE setting.key = 'auth.mfa_policy'
  AND setting.value ->> 'enforcement' = 'selected_users';

UPDATE system_settings
SET value = (value - 'selected_user_ids') || jsonb_build_object(
    'enforcement', CASE
        WHEN value ->> 'enforcement' = 'selected_users' THEN 'none'
        ELSE COALESCE(value ->> 'enforcement', 'none')
    END,
    'minimum_factors', CASE
        WHEN value ? 'minimum_factors' THEN value -> 'minimum_factors'
        WHEN value ->> 'enforcement' IN ('platform_admins', 'all_users')
            THEN '1'::jsonb
        ELSE '0'::jsonb
    END,
    'required_factors', COALESCE(value -> 'required_factors', '[]'::jsonb)
)
WHERE key = 'auth.mfa_policy'
  AND jsonb_typeof(value) = 'object';
"#;

pub(crate) const DOWN_SQL: &str = r#"
WITH selected_users AS (
    SELECT COALESCE(jsonb_agg(user_id ORDER BY user_id), '[]'::jsonb) AS ids
    FROM user_mfa_policies
    WHERE inherit_platform = false
      AND (minimum_factors > 0 OR jsonb_array_length(required_factors) > 0)
)
UPDATE system_settings
SET value = (value - 'minimum_factors' - 'required_factors') || jsonb_build_object(
    'enforcement', CASE
        WHEN value ->> 'enforcement' = 'none'
            AND (SELECT jsonb_array_length(ids) FROM selected_users) > 0
            THEN 'selected_users'
        ELSE COALESCE(value ->> 'enforcement', 'none')
    END,
    'selected_user_ids', CASE
        WHEN value ->> 'enforcement' = 'none' THEN (SELECT ids FROM selected_users)
        ELSE '[]'::jsonb
    END
)
WHERE key = 'auth.mfa_policy'
  AND jsonb_typeof(value) = 'object';

DROP TABLE user_mfa_policies;
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_creates_user_mfa_policy_constraints() {
        assert!(UP_SQL.contains("CREATE TABLE user_mfa_policies"));
        assert!(UP_SQL.contains("minimum_factors SMALLINT"));
        assert!(UP_SQL.contains("required_factors JSONB"));
        assert!(UP_SQL.contains("REFERENCES users(id) ON DELETE CASCADE"));
        assert!(UP_SQL.contains("jsonb_array_elements_text"));
        assert!(UP_SQL.contains("value - 'selected_user_ids'"));
    }

    #[test]
    fn migration_is_reversible() {
        assert!(DOWN_SQL.contains("DROP TABLE user_mfa_policies"));
        assert!(DOWN_SQL.contains("'selected_user_ids'"));
    }
}
