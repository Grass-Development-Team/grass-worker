use sea_orm::entity::prelude::*;

macro_rules! string_active_enum {
    ($name:ident, $enum_name:literal, { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
        #[sea_orm(rs_type = "String", db_type = "Enum", enum_name = $enum_name)]
        pub enum $name {
            $(#[sea_orm(string_value = $value)] $variant,)+
        }
    };
}

string_active_enum!(UserStatus, "user_status", {
    Active => "active",
    Disabled => "disabled",
});

string_active_enum!(PlatformRole, "platform_role", {
    User => "user",
    Admin => "admin",
});

string_active_enum!(IdentityProviderKind, "identity_provider_kind", {
    Oidc => "oidc",
    Github => "github",
});

impl IdentityProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oidc => "oidc",
            Self::Github => "github",
        }
    }
}

string_active_enum!(AuthTokenKind, "auth_token_kind", {
    EmailVerification => "email_verification",
    PasswordReset => "password_reset",
});

string_active_enum!(MfaFactorKind, "mfa_factor_kind", {
    Totp => "totp",
    Email => "email",
});

impl MfaFactorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::Email => "email",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "totp" => Some(Self::Totp),
            "email" => Some(Self::Email),
            _ => None,
        }
    }
}

impl PlatformRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

impl UserStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

impl TeamKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Team => "team",
        }
    }
}

string_active_enum!(TeamKind, "team_kind", {
    Personal => "personal",
    Team => "team",
});

string_active_enum!(TeamMemberRole, "team_member_role", {
    Owner => "owner",
    Admin => "admin",
    Member => "member",
    Viewer => "viewer",
});

string_active_enum!(TeamInvitationStatus, "team_invitation_status", {
    Pending => "pending",
    Accepted => "accepted",
    Expired => "expired",
    Revoked => "revoked",
});

string_active_enum!(ProjectRuntime, "project_runtime", {
    Static => "static",
    Ssr => "ssr",
    Hybrid => "hybrid",
    Serverless => "serverless",
    Edge => "edge",
});

string_active_enum!(DeploymentEnvironment, "deployment_environment", {
    Production => "production",
    Preview => "preview",
});

string_active_enum!(DeploymentBuildStatus, "deployment_build_status", {
    Pending => "pending",
    Claimed => "claimed",
    Queued => "queued",
    Building => "building",
    Ready => "ready",
    Failed => "failed",
    Canceled => "canceled",
});

string_active_enum!(DeploymentReleaseStatus, "deployment_release_status", {
    Draft => "draft",
    PendingReview => "pending_review",
    Approved => "approved",
    Rejected => "rejected",
    Active => "active",
});

string_active_enum!(DeploymentServeStatus, "deployment_serve_status", {
    Pending => "pending",
    Syncing => "syncing",
    Ready => "ready",
    Failed => "failed",
    Retired => "retired",
});

string_active_enum!(DeploymentEventKind, "deployment_event_kind", {
    System => "system",
    Build => "build",
    Serve => "serve",
    Release => "release",
    Review => "review",
    Host => "host",
});

string_active_enum!(DeploymentArtifactKind, "deployment_artifact_kind", {
    GrassOutput => "grass_output",
    BuildLog => "build_log",
    StaticSite => "static_site",
    Screenshot => "screenshot",
});

string_active_enum!(DeploymentScreenshotStatus, "deployment_screenshot_status", {
    Pending => "pending",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
});

string_active_enum!(StorageMigrationStatus, "storage_migration_status", {
    Pending => "pending",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
});

string_active_enum!(StorageMigrationObjectStatus, "storage_migration_object_status", {
    Pending => "pending",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
});

string_active_enum!(DeploymentReviewStatus, "deployment_review_status", {
    Pending => "pending",
    Approved => "approved",
    Rejected => "rejected",
});

string_active_enum!(ReleaseReason, "release_reason", {
    Auto => "auto",
    Promote => "promote",
    Rollback => "rollback",
});

string_active_enum!(QuotaEventKind, "quota_event_kind", {
    Reserve => "reserve",
    Consume => "consume",
    Release => "release",
    Deny => "deny",
    Adjust => "adjust",
});

string_active_enum!(QuotaPeriod, "quota_period", {
    None => "none",
    Monthly => "monthly",
});

string_active_enum!(HostSourceKind, "host_source_kind", {
    Wildcard => "wildcard",
    DnsProvider => "dns_provider",
    Manual => "manual",
});

string_active_enum!(HostBindingKind, "host_binding_kind", {
    Platform => "platform",
    Custom => "custom",
});

string_active_enum!(HostBindingEnvironment, "host_binding_environment", {
    Production => "production",
    Preview => "preview",
    All => "all",
});

string_active_enum!(HostBindingStatus, "host_binding_status", {
    Pending => "pending",
    Active => "active",
    Failed => "failed",
    Disabled => "disabled",
});

string_active_enum!(HostReviewStatus, "host_review_status", {
    NotRequired => "not_required",
    Pending => "pending",
    Approved => "approved",
    Rejected => "rejected",
});

string_active_enum!(HostProvisionEventStatus, "host_provision_event_status", {
    Success => "success",
    Pending => "pending",
    Failed => "failed",
});

string_active_enum!(NodeStatus, "node_status", {
    Pending => "pending",
    Active => "active",
    Draining => "draining",
    Offline => "offline",
    Disabled => "disabled",
});

string_active_enum!(NodeConfigSyncStatus, "node_config_sync_status", {
    Pending => "pending",
    Applying => "applying",
    Applied => "applied",
    Failed => "failed",
});

string_active_enum!(NodeDeletionStatus, "node_deletion_status", {
    Queued => "queued",
    Migrating => "migrating",
    Draining => "draining",
    Deleting => "deleting",
    Failed => "failed",
    Completed => "completed",
});

string_active_enum!(NodeDeploymentMigrationStatus, "node_deployment_migration_status", {
    Pending => "pending",
    Syncing => "syncing",
    Ready => "ready",
    Failed => "failed",
});

string_active_enum!(SystemSettingValueKind, "system_setting_value_kind", {
    String => "string",
    Number => "number",
    Boolean => "boolean",
    Json => "json",
    SecretRef => "secret_ref",
});

string_active_enum!(AuditEventResult, "audit_event_result", {
    Success => "success",
    Failure => "failure",
    Denied => "denied",
});

string_active_enum!(AuditActorType, "audit_actor_type", {
    Anonymous => "anonymous",
    User => "user",
    System => "system",
    Node => "node",
});

string_active_enum!(AuditEventVisibility, "audit_event_visibility", {
    Platform => "platform",
    Team => "team",
});

string_active_enum!(SourceCredentialKind, "source_credential_kind", {
    Https => "https",
    Ssh => "ssh",
});

impl SourceCredentialKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Https => "https",
            Self::Ssh => "ssh",
        }
    }
}

string_active_enum!(SshHostKeyStatus, "ssh_host_key_status", {
    Pending => "pending",
    Approved => "approved",
    Rejected => "rejected",
    Superseded => "superseded",
});
