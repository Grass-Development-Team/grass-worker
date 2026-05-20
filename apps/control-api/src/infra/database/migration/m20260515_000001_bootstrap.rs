use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(r#"CREATE EXTENSION IF NOT EXISTS "pgcrypto""#)
            .await?;

        create_enum_types(manager).await?;
        create_identity_tables(manager).await?;
        create_deployment_tables(manager).await?;
        create_quota_host_tables(manager).await?;
        create_node_audit_setting_tables(manager).await?;
        create_indexes(manager).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_indexes(manager).await?;

        for table in [
            "host_provision_events",
            "host_policies",
            "project_host_bindings",
            "host_sources",
            "quota_events",
            "quota_usage_counters",
            "quota_limits",
            "releases",
            "deployment_reviews",
            "deployment_artifacts",
            "deployment_events",
            "deployments",
            "projects",
            "system_settings",
            "audit_events",
            "nodes",
            "team_invitations",
            "team_members",
            "user_password_credentials",
            "teams",
            "team_groups",
            "quota_plans",
            "users",
        ] {
            manager
                .drop_table(
                    Table::drop()
                        .table(Alias::new(table))
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }

        drop_enum_types(manager).await?;

        Ok(())
    }
}

async fn create_enum_types(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for (name, values) in [
        ("user_status", &["active", "disabled"][..]),
        ("team_kind", &["personal", "team"]),
        ("team_member_role", &["owner", "admin", "member", "viewer"]),
        (
            "team_invitation_status",
            &["pending", "accepted", "expired", "revoked"],
        ),
        (
            "project_runtime",
            &["static", "ssr", "hybrid", "serverless", "edge"],
        ),
        ("deployment_environment", &["production", "preview"]),
        (
            "deployment_build_status",
            &[
                "pending", "claimed", "queued", "building", "ready", "failed", "canceled",
            ],
        ),
        (
            "deployment_release_status",
            &["draft", "pending_review", "approved", "rejected", "active"],
        ),
        (
            "deployment_event_kind",
            &["system", "build", "release", "review", "host"],
        ),
        (
            "deployment_artifact_kind",
            &["grass_output", "build_log", "static_site"],
        ),
        (
            "deployment_review_status",
            &["pending", "approved", "rejected"],
        ),
        ("release_reason", &["auto", "promote", "rollback"]),
        (
            "quota_event_kind",
            &["reserve", "consume", "release", "deny", "adjust"],
        ),
        ("quota_period", &["none", "monthly"]),
        ("host_source_kind", &["wildcard", "dns_provider", "manual"]),
        ("host_binding_kind", &["platform", "custom"]),
        (
            "host_binding_environment",
            &["production", "preview", "all"],
        ),
        (
            "host_binding_status",
            &["pending", "active", "failed", "disabled"],
        ),
        (
            "host_provision_event_status",
            &["success", "pending", "failed"],
        ),
        (
            "node_status",
            &["pending", "active", "draining", "offline", "disabled"],
        ),
        (
            "system_setting_value_kind",
            &["string", "number", "boolean", "json", "secret_ref"],
        ),
        ("audit_event_result", &["success", "failure", "denied"]),
    ] {
        create_enum_type(manager, name, values).await?;
    }

    Ok(())
}

async fn create_enum_type(
    manager: &SchemaManager<'_>,
    name: &str,
    values: &[&str],
) -> Result<(), DbErr> {
    let quoted_values = values
        .iter()
        .map(|value| format!("'{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "DO $$ BEGIN CREATE TYPE {name} AS ENUM ({quoted_values}); EXCEPTION WHEN duplicate_object THEN NULL; END $$;"
    );
    manager.get_connection().execute_unprepared(&sql).await?;
    Ok(())
}

async fn drop_enum_types(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for name in [
        "audit_event_result",
        "system_setting_value_kind",
        "node_status",
        "host_provision_event_status",
        "host_binding_status",
        "host_binding_environment",
        "host_binding_kind",
        "host_source_kind",
        "quota_period",
        "quota_event_kind",
        "release_reason",
        "deployment_review_status",
        "deployment_artifact_kind",
        "deployment_event_kind",
        "deployment_release_status",
        "deployment_build_status",
        "deployment_environment",
        "project_runtime",
        "team_invitation_status",
        "team_member_role",
        "team_kind",
        "user_status",
    ] {
        manager
            .get_connection()
            .execute_unprepared(&format!("DROP TYPE IF EXISTS {name}"))
            .await?;
    }

    Ok(())
}

async fn create_identity_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Users::Table)
                .if_not_exists()
                .col(uuid_pk(Users::Id))
                .col(string_len(Users::Email, 320).not_null())
                .col(string_len_null(Users::DisplayName, 120))
                .col(enum_col(Users::Status, "user_status", "active"))
                .col(nullable_timestamp_col(Users::LastLoginAt))
                .col(timestamp_col(Users::CreatedAt))
                .col(timestamp_col(Users::UpdatedAt))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(QuotaPlans::Table)
                .if_not_exists()
                .col(uuid_pk(QuotaPlans::Id))
                .col(string_len(QuotaPlans::Code, 80).not_null())
                .col(string_len(QuotaPlans::Name, 120).not_null())
                .col(text_null(QuotaPlans::Description))
                .col(boolean(QuotaPlans::IsDefault).not_null().default(false))
                .col(boolean(QuotaPlans::Enabled).not_null().default(true))
                .col(timestamp_col(QuotaPlans::CreatedAt))
                .col(timestamp_col(QuotaPlans::UpdatedAt))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(TeamGroups::Table)
                .if_not_exists()
                .col(uuid_pk(TeamGroups::Id))
                .col(string_len(TeamGroups::Code, 80).not_null())
                .col(string_len(TeamGroups::Name, 120).not_null())
                .col(text_null(TeamGroups::Description))
                .col(uuid_null(TeamGroups::QuotaPlanId))
                .col(boolean(TeamGroups::IsDefault).not_null().default(false))
                .col(timestamp_col(TeamGroups::CreatedAt))
                .col(timestamp_col(TeamGroups::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_team_groups_quota_plan_id")
                        .from(TeamGroups::Table, TeamGroups::QuotaPlanId)
                        .to(QuotaPlans::Table, QuotaPlans::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Teams::Table)
                .if_not_exists()
                .col(uuid_pk(Teams::Id))
                .col(string_len(Teams::Slug, 120).not_null())
                .col(string_len(Teams::Name, 160).not_null())
                .col(enum_col(Teams::Kind, "team_kind", "team"))
                .col(uuid_null(Teams::GroupId))
                .col(uuid_null(Teams::ExplicitQuotaPlanId))
                .col(uuid_null(Teams::OwnerUserId))
                .col(timestamp_col(Teams::CreatedAt))
                .col(timestamp_col(Teams::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_teams_group_id")
                        .from(Teams::Table, Teams::GroupId)
                        .to(TeamGroups::Table, TeamGroups::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_teams_explicit_quota_plan_id")
                        .from(Teams::Table, Teams::ExplicitQuotaPlanId)
                        .to(QuotaPlans::Table, QuotaPlans::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_teams_owner_user_id")
                        .from(Teams::Table, Teams::OwnerUserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(UserPasswordCredentials::Table)
                .if_not_exists()
                .col(uuid_pk(UserPasswordCredentials::Id))
                .col(uuid(UserPasswordCredentials::UserId).not_null())
                .col(text(UserPasswordCredentials::PasswordHash).not_null())
                .col(
                    boolean(UserPasswordCredentials::MustChangePassword)
                        .not_null()
                        .default(false),
                )
                .col(timestamp_col(UserPasswordCredentials::CreatedAt))
                .col(timestamp_col(UserPasswordCredentials::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_user_password_credentials_user_id")
                        .from(
                            UserPasswordCredentials::Table,
                            UserPasswordCredentials::UserId,
                        )
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(TeamMembers::Table)
                .if_not_exists()
                .col(uuid_pk(TeamMembers::Id))
                .col(uuid(TeamMembers::TeamId).not_null())
                .col(uuid(TeamMembers::UserId).not_null())
                .col(enum_col(TeamMembers::Role, "team_member_role", "member"))
                .col(uuid_null(TeamMembers::InvitedByUserId))
                .col(timestamp_col(TeamMembers::JoinedAt))
                .col(timestamp_col(TeamMembers::CreatedAt))
                .col(timestamp_col(TeamMembers::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_team_members_team_id")
                        .from(TeamMembers::Table, TeamMembers::TeamId)
                        .to(Teams::Table, Teams::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_team_members_user_id")
                        .from(TeamMembers::Table, TeamMembers::UserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_team_members_invited_by_user_id")
                        .from(TeamMembers::Table, TeamMembers::InvitedByUserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(TeamInvitations::Table)
                .if_not_exists()
                .col(uuid_pk(TeamInvitations::Id))
                .col(uuid(TeamInvitations::TeamId).not_null())
                .col(string_len(TeamInvitations::Email, 320).not_null())
                .col(enum_col(
                    TeamInvitations::Role,
                    "team_member_role",
                    "member",
                ))
                .col(enum_col(
                    TeamInvitations::Status,
                    "team_invitation_status",
                    "pending",
                ))
                .col(uuid_null(TeamInvitations::InvitedByUserId))
                .col(timestamp_col(TeamInvitations::ExpiresAt))
                .col(nullable_timestamp_col(TeamInvitations::AcceptedAt))
                .col(timestamp_col(TeamInvitations::CreatedAt))
                .col(timestamp_col(TeamInvitations::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_team_invitations_team_id")
                        .from(TeamInvitations::Table, TeamInvitations::TeamId)
                        .to(Teams::Table, Teams::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_team_invitations_invited_by_user_id")
                        .from(TeamInvitations::Table, TeamInvitations::InvitedByUserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    Ok(())
}

async fn create_deployment_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Nodes::Table)
                .if_not_exists()
                .col(uuid_pk(Nodes::Id))
                .col(string_len(Nodes::Name, 120).not_null())
                .col(text(Nodes::TokenHash).not_null())
                .col(enum_col(Nodes::Status, "node_status", "pending"))
                .col(boolean(Nodes::BuildEnabled).not_null().default(true))
                .col(boolean(Nodes::ServeEnabled).not_null().default(true))
                .col(integer(Nodes::BuildConcurrency).not_null().default(1))
                .col(text_null(Nodes::BaseUrl))
                .col(text_null(Nodes::WorkRoot))
                .col(json_binary(Nodes::Metadata).not_null().default("{}"))
                .col(nullable_timestamp_col(Nodes::LastHeartbeatAt))
                .col(timestamp_col(Nodes::CreatedAt))
                .col(timestamp_col(Nodes::UpdatedAt))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Projects::Table)
                .if_not_exists()
                .col(uuid_pk(Projects::Id))
                .col(uuid(Projects::TeamId).not_null())
                .col(string_len(Projects::Slug, 120).not_null())
                .col(string_len(Projects::Name, 160).not_null())
                .col(enum_col(Projects::Runtime, "project_runtime", "static"))
                .col(text_null(Projects::RepositoryUrl))
                .col(string_len_null(Projects::DefaultBranch, 160))
                .col(text_null(Projects::InstallCommand))
                .col(text_null(Projects::BuildCommand))
                .col(text_null(Projects::OutputDirectory))
                .col(json_binary(Projects::SourceConfig).not_null().default("{}"))
                .col(json_binary(Projects::BuildConfig).not_null().default("{}"))
                .col(timestamp_col(Projects::CreatedAt))
                .col(timestamp_col(Projects::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_projects_team_id")
                        .from(Projects::Table, Projects::TeamId)
                        .to(Teams::Table, Teams::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Deployments::Table)
                .if_not_exists()
                .col(uuid_pk(Deployments::Id))
                .col(uuid(Deployments::ProjectId).not_null())
                .col(uuid(Deployments::TeamId).not_null())
                .col(uuid_null(Deployments::NodeId))
                .col(enum_col(
                    Deployments::Environment,
                    "deployment_environment",
                    "preview",
                ))
                .col(enum_col(
                    Deployments::BuildStatus,
                    "deployment_build_status",
                    "pending",
                ))
                .col(enum_col(
                    Deployments::ReleaseStatus,
                    "deployment_release_status",
                    "draft",
                ))
                .col(text_null(Deployments::SourceRepositoryUrl))
                .col(string_len_null(Deployments::SourceBranch, 160))
                .col(string_len_null(Deployments::CommitHash, 80))
                .col(text_null(Deployments::CommitMessage))
                .col(uuid_null(Deployments::TriggeredByUserId))
                .col(text_null(Deployments::InstallCommand))
                .col(text_null(Deployments::BuildCommand))
                .col(text_null(Deployments::OutputDirectory))
                .col(
                    json_binary(Deployments::SourceMetadata)
                        .not_null()
                        .default("{}"),
                )
                .col(text_null(Deployments::FailureCode))
                .col(text_null(Deployments::FailureMessage))
                .col(nullable_timestamp_col(Deployments::ClaimedAt))
                .col(nullable_timestamp_col(Deployments::BuildStartedAt))
                .col(nullable_timestamp_col(Deployments::BuildFinishedAt))
                .col(timestamp_col(Deployments::CreatedAt))
                .col(timestamp_col(Deployments::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deployments_project_id")
                        .from(Deployments::Table, Deployments::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deployments_team_id")
                        .from(Deployments::Table, Deployments::TeamId)
                        .to(Teams::Table, Teams::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deployments_node_id")
                        .from(Deployments::Table, Deployments::NodeId)
                        .to(Nodes::Table, Nodes::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deployments_triggered_by_user_id")
                        .from(Deployments::Table, Deployments::TriggeredByUserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(DeploymentEvents::Table)
                .if_not_exists()
                .col(uuid_pk(DeploymentEvents::Id))
                .col(uuid(DeploymentEvents::DeploymentId).not_null())
                .col(enum_col(
                    DeploymentEvents::Kind,
                    "deployment_event_kind",
                    "system",
                ))
                .col(text(DeploymentEvents::Message).not_null())
                .col(
                    json_binary(DeploymentEvents::Metadata)
                        .not_null()
                        .default("{}"),
                )
                .col(timestamp_col(DeploymentEvents::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deployment_events_deployment_id")
                        .from(DeploymentEvents::Table, DeploymentEvents::DeploymentId)
                        .to(Deployments::Table, Deployments::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(DeploymentArtifacts::Table)
                .if_not_exists()
                .col(uuid_pk(DeploymentArtifacts::Id))
                .col(uuid(DeploymentArtifacts::DeploymentId).not_null())
                .col(enum_col(
                    DeploymentArtifacts::Kind,
                    "deployment_artifact_kind",
                    "grass_output",
                ))
                .col(text(DeploymentArtifacts::StoragePath).not_null())
                .col(text_null(DeploymentArtifacts::ChecksumSha256))
                .col(big_integer_null(DeploymentArtifacts::SizeBytes))
                .col(
                    json_binary(DeploymentArtifacts::Manifest)
                        .not_null()
                        .default("{}"),
                )
                .col(timestamp_col(DeploymentArtifacts::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deployment_artifacts_deployment_id")
                        .from(
                            DeploymentArtifacts::Table,
                            DeploymentArtifacts::DeploymentId,
                        )
                        .to(Deployments::Table, Deployments::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(DeploymentReviews::Table)
                .if_not_exists()
                .col(uuid_pk(DeploymentReviews::Id))
                .col(uuid(DeploymentReviews::DeploymentId).not_null())
                .col(uuid_null(DeploymentReviews::ReviewerUserId))
                .col(enum_col(
                    DeploymentReviews::Status,
                    "deployment_review_status",
                    "pending",
                ))
                .col(text_null(DeploymentReviews::Reason))
                .col(timestamp_col(DeploymentReviews::RequestedAt))
                .col(nullable_timestamp_col(DeploymentReviews::ReviewedAt))
                .col(timestamp_col(DeploymentReviews::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deployment_reviews_deployment_id")
                        .from(DeploymentReviews::Table, DeploymentReviews::DeploymentId)
                        .to(Deployments::Table, Deployments::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deployment_reviews_reviewer_user_id")
                        .from(DeploymentReviews::Table, DeploymentReviews::ReviewerUserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(Releases::Table)
                .if_not_exists()
                .col(uuid_pk(Releases::Id))
                .col(uuid(Releases::ProjectId).not_null())
                .col(uuid(Releases::DeploymentId).not_null())
                .col(enum_col(
                    Releases::Environment,
                    "deployment_environment",
                    "production",
                ))
                .col(enum_col(Releases::Reason, "release_reason", "auto"))
                .col(uuid_null(Releases::ActorUserId))
                .col(uuid_null(Releases::PreviousDeploymentId))
                .col(timestamp_col(Releases::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_releases_project_id")
                        .from(Releases::Table, Releases::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_releases_deployment_id")
                        .from(Releases::Table, Releases::DeploymentId)
                        .to(Deployments::Table, Deployments::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_releases_actor_user_id")
                        .from(Releases::Table, Releases::ActorUserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_releases_previous_deployment_id")
                        .from(Releases::Table, Releases::PreviousDeploymentId)
                        .to(Deployments::Table, Deployments::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    Ok(())
}

async fn create_quota_host_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(QuotaLimits::Table)
                .if_not_exists()
                .col(uuid_pk(QuotaLimits::Id))
                .col(uuid(QuotaLimits::QuotaPlanId).not_null())
                .col(string_len(QuotaLimits::Dimension, 120).not_null())
                .col(big_integer(QuotaLimits::LimitValue).not_null())
                .col(enum_col(QuotaLimits::Period, "quota_period", "none"))
                .col(timestamp_col(QuotaLimits::CreatedAt))
                .col(timestamp_col(QuotaLimits::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_quota_limits_quota_plan_id")
                        .from(QuotaLimits::Table, QuotaLimits::QuotaPlanId)
                        .to(QuotaPlans::Table, QuotaPlans::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(QuotaUsageCounters::Table)
                .if_not_exists()
                .col(uuid_pk(QuotaUsageCounters::Id))
                .col(uuid(QuotaUsageCounters::TeamId).not_null())
                .col(string_len(QuotaUsageCounters::Dimension, 120).not_null())
                .col(
                    big_integer(QuotaUsageCounters::UsedValue)
                        .not_null()
                        .default(0),
                )
                .col(nullable_timestamp_col(QuotaUsageCounters::PeriodStart))
                .col(nullable_timestamp_col(QuotaUsageCounters::PeriodEnd))
                .col(timestamp_col(QuotaUsageCounters::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_quota_usage_counters_team_id")
                        .from(QuotaUsageCounters::Table, QuotaUsageCounters::TeamId)
                        .to(Teams::Table, Teams::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(QuotaEvents::Table)
                .if_not_exists()
                .col(uuid_pk(QuotaEvents::Id))
                .col(uuid(QuotaEvents::TeamId).not_null())
                .col(string_len(QuotaEvents::Dimension, 120).not_null())
                .col(enum_col(QuotaEvents::Kind, "quota_event_kind", "consume"))
                .col(big_integer(QuotaEvents::DeltaValue).not_null())
                .col(string_len_null(QuotaEvents::IdempotencyKey, 160))
                .col(text_null(QuotaEvents::ResourceType))
                .col(uuid_null(QuotaEvents::ResourceId))
                .col(json_binary(QuotaEvents::Metadata).not_null().default("{}"))
                .col(timestamp_col(QuotaEvents::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_quota_events_team_id")
                        .from(QuotaEvents::Table, QuotaEvents::TeamId)
                        .to(Teams::Table, Teams::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(HostSources::Table)
                .if_not_exists()
                .col(uuid_pk(HostSources::Id))
                .col(enum_col(HostSources::Kind, "host_source_kind", "wildcard"))
                .col(string_len(HostSources::Label, 120).not_null())
                .col(string_len(HostSources::BaseDomain, 253).not_null())
                .col(boolean(HostSources::Enabled).not_null().default(true))
                .col(
                    boolean(HostSources::AllowsAutoAssign)
                        .not_null()
                        .default(true),
                )
                .col(boolean(HostSources::IsDefault).not_null().default(false))
                .col(string_len_null(HostSources::Provider, 80))
                .col(json_binary(HostSources::Config).not_null().default("{}"))
                .col(timestamp_col(HostSources::CreatedAt))
                .col(timestamp_col(HostSources::UpdatedAt))
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(ProjectHostBindings::Table)
                .if_not_exists()
                .col(uuid_pk(ProjectHostBindings::Id))
                .col(uuid(ProjectHostBindings::ProjectId).not_null())
                .col(uuid(ProjectHostBindings::TeamId).not_null())
                .col(uuid_null(ProjectHostBindings::HostSourceId))
                .col(string_len(ProjectHostBindings::Host, 253).not_null())
                .col(enum_col(
                    ProjectHostBindings::Kind,
                    "host_binding_kind",
                    "platform",
                ))
                .col(enum_col(
                    ProjectHostBindings::Environment,
                    "host_binding_environment",
                    "all",
                ))
                .col(enum_col(
                    ProjectHostBindings::Status,
                    "host_binding_status",
                    "pending",
                ))
                .col(text_null(ProjectHostBindings::FailureReason))
                .col(timestamp_col(ProjectHostBindings::CreatedAt))
                .col(timestamp_col(ProjectHostBindings::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_project_host_bindings_project_id")
                        .from(ProjectHostBindings::Table, ProjectHostBindings::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_project_host_bindings_team_id")
                        .from(ProjectHostBindings::Table, ProjectHostBindings::TeamId)
                        .to(Teams::Table, Teams::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_project_host_bindings_host_source_id")
                        .from(
                            ProjectHostBindings::Table,
                            ProjectHostBindings::HostSourceId,
                        )
                        .to(HostSources::Table, HostSources::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(HostPolicies::Table)
                .if_not_exists()
                .col(uuid_pk(HostPolicies::Id))
                .col(uuid_null(HostPolicies::TeamGroupId))
                .col(uuid_null(HostPolicies::QuotaPlanId))
                .col(integer(HostPolicies::MaxHosts).not_null().default(0))
                .col(
                    boolean(HostPolicies::AllowCustomHosts)
                        .not_null()
                        .default(false),
                )
                .col(
                    boolean(HostPolicies::AllowAutoAssign)
                        .not_null()
                        .default(true),
                )
                .col(timestamp_col(HostPolicies::CreatedAt))
                .col(timestamp_col(HostPolicies::UpdatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_host_policies_team_group_id")
                        .from(HostPolicies::Table, HostPolicies::TeamGroupId)
                        .to(TeamGroups::Table, TeamGroups::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_host_policies_quota_plan_id")
                        .from(HostPolicies::Table, HostPolicies::QuotaPlanId)
                        .to(QuotaPlans::Table, QuotaPlans::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(HostProvisionEvents::Table)
                .if_not_exists()
                .col(uuid_pk(HostProvisionEvents::Id))
                .col(uuid(HostProvisionEvents::HostBindingId).not_null())
                .col(uuid_null(HostProvisionEvents::HostSourceId))
                .col(enum_col(
                    HostProvisionEvents::Status,
                    "host_provision_event_status",
                    "pending",
                ))
                .col(text(HostProvisionEvents::Operation).not_null())
                .col(text_null(HostProvisionEvents::ProviderRequestId))
                .col(text_null(HostProvisionEvents::ErrorCode))
                .col(text_null(HostProvisionEvents::ErrorMessage))
                .col(
                    json_binary(HostProvisionEvents::Metadata)
                        .not_null()
                        .default("{}"),
                )
                .col(timestamp_col(HostProvisionEvents::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_host_provision_events_host_binding_id")
                        .from(
                            HostProvisionEvents::Table,
                            HostProvisionEvents::HostBindingId,
                        )
                        .to(ProjectHostBindings::Table, ProjectHostBindings::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_host_provision_events_host_source_id")
                        .from(
                            HostProvisionEvents::Table,
                            HostProvisionEvents::HostSourceId,
                        )
                        .to(HostSources::Table, HostSources::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    Ok(())
}

async fn create_node_audit_setting_tables(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AuditEvents::Table)
                .if_not_exists()
                .col(uuid_pk(AuditEvents::Id))
                .col(uuid_null(AuditEvents::ActorUserId))
                .col(text(AuditEvents::Action).not_null())
                .col(text(AuditEvents::TargetType).not_null())
                .col(uuid_null(AuditEvents::TargetId))
                .col(enum_col(
                    AuditEvents::Result,
                    "audit_event_result",
                    "success",
                ))
                .col(text_null(AuditEvents::Reason))
                .col(json_binary(AuditEvents::Metadata).not_null().default("{}"))
                .col(timestamp_col(AuditEvents::CreatedAt))
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_audit_events_actor_user_id")
                        .from(AuditEvents::Table, AuditEvents::ActorUserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_table(
            Table::create()
                .table(SystemSettings::Table)
                .if_not_exists()
                .col(uuid_pk(SystemSettings::Id))
                .col(string_len(SystemSettings::Key, 160).not_null())
                .col(enum_col(
                    SystemSettings::ValueKind,
                    "system_setting_value_kind",
                    "string",
                ))
                .col(json_binary(SystemSettings::Value).not_null())
                .col(boolean(SystemSettings::IsSecret).not_null().default(false))
                .col(timestamp_col(SystemSettings::CreatedAt))
                .col(timestamp_col(SystemSettings::UpdatedAt))
                .to_owned(),
        )
        .await?;

    Ok(())
}

async fn create_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for (name, table, columns, unique) in [
        ("ux_users_email", "users", "email", true),
        ("ux_quota_plans_code", "quota_plans", "code", true),
        ("ux_team_groups_code", "team_groups", "code", true),
        ("ux_teams_slug", "teams", "slug", true),
        (
            "ux_user_password_credentials_user_id",
            "user_password_credentials",
            "user_id",
            true,
        ),
        (
            "ux_team_members_team_user",
            "team_members",
            "team_id, user_id",
            true,
        ),
        (
            "ux_team_invitations_team_email",
            "team_invitations",
            "team_id, email",
            true,
        ),
        ("ux_nodes_name", "nodes", "name", true),
        ("ux_projects_team_slug", "projects", "team_id, slug", true),
        (
            "ix_deployments_project_created",
            "deployments",
            "project_id, created_at",
            false,
        ),
        (
            "ux_quota_limits_plan_dimension_period",
            "quota_limits",
            "quota_plan_id, dimension, period",
            true,
        ),
        (
            "ux_quota_usage_counters_team_dimension_period",
            "quota_usage_counters",
            "team_id, dimension, period_start, period_end",
            true,
        ),
        (
            "ux_quota_events_idempotency_key",
            "quota_events",
            "idempotency_key",
            true,
        ),
        (
            "ux_project_host_bindings_host",
            "project_host_bindings",
            "host",
            true,
        ),
        ("ux_system_settings_key", "system_settings", "key", true),
    ] {
        let unique = if unique { "UNIQUE " } else { "" };
        manager
            .get_connection()
            .execute_unprepared(&format!(
                "CREATE {unique}INDEX IF NOT EXISTS {name} ON {table} ({columns})"
            ))
            .await?;
    }

    for sql in [
        "CREATE UNIQUE INDEX IF NOT EXISTS ux_deployments_one_active_per_project_environment ON deployments (project_id, environment) WHERE release_status = 'active'",
        "CREATE UNIQUE INDEX IF NOT EXISTS ux_host_sources_single_enabled_default ON host_sources (is_default) WHERE enabled = true AND is_default = true",
        "CREATE UNIQUE INDEX IF NOT EXISTS ux_team_groups_single_default ON team_groups (is_default) WHERE is_default = true",
        "CREATE UNIQUE INDEX IF NOT EXISTS ux_quota_plans_single_default ON quota_plans (is_default) WHERE is_default = true",
    ] {
        manager.get_connection().execute_unprepared(sql).await?;
    }

    Ok(())
}

async fn drop_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for name in [
        "ux_quota_plans_single_default",
        "ux_team_groups_single_default",
        "ux_host_sources_single_enabled_default",
        "ux_deployments_one_active_per_project_environment",
        "ux_system_settings_key",
        "ux_project_host_bindings_host",
        "ux_quota_events_idempotency_key",
        "ux_quota_usage_counters_team_dimension_period",
        "ux_quota_limits_plan_dimension_period",
        "ix_deployments_project_created",
        "ux_projects_team_slug",
        "ux_nodes_name",
        "ux_team_invitations_team_email",
        "ux_team_members_team_user",
        "ux_user_password_credentials_user_id",
        "ux_teams_slug",
        "ux_team_groups_code",
        "ux_quota_plans_code",
        "ux_users_email",
    ] {
        manager
            .get_connection()
            .execute_unprepared(&format!("DROP INDEX IF EXISTS {name}"))
            .await?;
    }

    Ok(())
}

fn uuid_pk<T: IntoIden>(name: T) -> ColumnDef {
    let mut column = ColumnDef::new(name);
    column
        .uuid()
        .not_null()
        .primary_key()
        .default(Expr::cust("gen_random_uuid()"));
    column
}

fn enum_col<T: IntoIden>(name: T, enum_type: &str, default_value: &str) -> ColumnDef {
    let mut column = ColumnDef::new(name);
    column
        .custom(Alias::new(enum_type))
        .not_null()
        .default(Expr::cust(format!("'{default_value}'::{enum_type}")));
    column
}

fn timestamp_col<T: IntoIden>(name: T) -> ColumnDef {
    let mut column = ColumnDef::new(name);
    column
        .timestamp_with_time_zone()
        .not_null()
        .default(Expr::cust("CURRENT_TIMESTAMP"));
    column
}

fn nullable_timestamp_col<T: IntoIden>(name: T) -> ColumnDef {
    let mut column = ColumnDef::new(name);
    column.timestamp_with_time_zone().null();
    column
}

fn json_binary<T: IntoIden>(name: T) -> ColumnDef {
    let mut column = ColumnDef::new(name);
    column.json_binary();
    column
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Email,
    DisplayName,
    Status,
    LastLoginAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum UserPasswordCredentials {
    Table,
    Id,
    UserId,
    PasswordHash,
    MustChangePassword,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum QuotaPlans {
    Table,
    Id,
    Code,
    Name,
    Description,
    IsDefault,
    Enabled,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TeamGroups {
    Table,
    Id,
    Code,
    Name,
    Description,
    QuotaPlanId,
    IsDefault,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Teams {
    Table,
    Id,
    Slug,
    Name,
    Kind,
    GroupId,
    ExplicitQuotaPlanId,
    OwnerUserId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TeamMembers {
    Table,
    Id,
    TeamId,
    UserId,
    Role,
    InvitedByUserId,
    JoinedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TeamInvitations {
    Table,
    Id,
    TeamId,
    Email,
    Role,
    Status,
    InvitedByUserId,
    ExpiresAt,
    AcceptedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Nodes {
    Table,
    Id,
    Name,
    TokenHash,
    Status,
    BuildEnabled,
    ServeEnabled,
    BuildConcurrency,
    BaseUrl,
    WorkRoot,
    Metadata,
    LastHeartbeatAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    TeamId,
    Slug,
    Name,
    Runtime,
    RepositoryUrl,
    DefaultBranch,
    InstallCommand,
    BuildCommand,
    OutputDirectory,
    SourceConfig,
    BuildConfig,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Deployments {
    Table,
    Id,
    ProjectId,
    TeamId,
    NodeId,
    Environment,
    BuildStatus,
    ReleaseStatus,
    SourceRepositoryUrl,
    SourceBranch,
    CommitHash,
    CommitMessage,
    TriggeredByUserId,
    InstallCommand,
    BuildCommand,
    OutputDirectory,
    SourceMetadata,
    FailureCode,
    FailureMessage,
    ClaimedAt,
    BuildStartedAt,
    BuildFinishedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum DeploymentEvents {
    Table,
    Id,
    DeploymentId,
    Kind,
    Message,
    Metadata,
    CreatedAt,
}

#[derive(DeriveIden)]
enum DeploymentArtifacts {
    Table,
    Id,
    DeploymentId,
    Kind,
    StoragePath,
    ChecksumSha256,
    SizeBytes,
    Manifest,
    CreatedAt,
}

#[derive(DeriveIden)]
enum DeploymentReviews {
    Table,
    Id,
    DeploymentId,
    ReviewerUserId,
    Status,
    Reason,
    RequestedAt,
    ReviewedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Releases {
    Table,
    Id,
    ProjectId,
    DeploymentId,
    Environment,
    Reason,
    ActorUserId,
    PreviousDeploymentId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum QuotaLimits {
    Table,
    Id,
    QuotaPlanId,
    Dimension,
    LimitValue,
    Period,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum QuotaUsageCounters {
    Table,
    Id,
    TeamId,
    Dimension,
    UsedValue,
    PeriodStart,
    PeriodEnd,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum QuotaEvents {
    Table,
    Id,
    TeamId,
    Dimension,
    Kind,
    DeltaValue,
    IdempotencyKey,
    ResourceType,
    ResourceId,
    Metadata,
    CreatedAt,
}

#[derive(DeriveIden)]
enum HostSources {
    Table,
    Id,
    Kind,
    Label,
    BaseDomain,
    Enabled,
    AllowsAutoAssign,
    IsDefault,
    Provider,
    Config,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ProjectHostBindings {
    Table,
    Id,
    ProjectId,
    TeamId,
    HostSourceId,
    Host,
    Kind,
    Environment,
    Status,
    FailureReason,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum HostPolicies {
    Table,
    Id,
    TeamGroupId,
    QuotaPlanId,
    MaxHosts,
    AllowCustomHosts,
    AllowAutoAssign,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum HostProvisionEvents {
    Table,
    Id,
    HostBindingId,
    HostSourceId,
    Status,
    Operation,
    ProviderRequestId,
    ErrorCode,
    ErrorMessage,
    Metadata,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AuditEvents {
    Table,
    Id,
    ActorUserId,
    Action,
    TargetType,
    TargetId,
    Result,
    Reason,
    Metadata,
    CreatedAt,
}

#[derive(DeriveIden)]
enum SystemSettings {
    Table,
    Id,
    Key,
    ValueKind,
    Value,
    IsSecret,
    CreatedAt,
    UpdatedAt,
}
