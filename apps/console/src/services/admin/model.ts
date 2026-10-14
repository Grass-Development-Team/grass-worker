import type { Announcement } from "@/services/announcements/index";
import type { BuildStatus, ReleaseStatus, ServeStatus } from "@/services/deployments/index";
import type { StorageConfiguration } from "@/services/storage/form";

export interface AdministrationStatus {
  service: string;
  mode: "ready";
  version: string;
}

export type NodeStatus = "pending" | "active" | "draining" | "offline" | "disabled";

export interface AdminNode {
  id: string;
  name: string;
  region: string;
  status: NodeStatus;
  healthy: boolean;
  build_enabled: boolean;
  serve_enabled: boolean;
  build_concurrency: number;
  base_url: string | null;
  work_root: string | null;
  version: string | null;
  capacity: AdminNodeResources;
  usage: AdminNodeUsage;
  overflow_count: number;
  deletion?: AdminNodeDeletionJob | null;
  configuration: AdminNodeConfigurationSync;
  last_heartbeat_at: string | null;
  created_at: string;
}

export type NodeDeletionStatus =
  "queued" | "migrating" | "draining" | "deleting" | "failed" | "completed";

export interface AdminNodeDeletionJob {
  id: string;
  status: NodeDeletionStatus;
  target_node_id: string | null;
  total_deployments: number;
  migrated_deployments: number;
  active_builds: number;
  error: string | null;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
}

export interface AdminNodeDeletionPlan {
  node_id: string;
  assigned_deployments: number;
  active_builds: number;
  requires_target: boolean;
  eligible_targets: Array<{
    id: string;
    name: string;
    available_deployments: number;
  }>;
}

export type NodeConfigurationSyncStatus = "pending" | "applying" | "applied" | "failed";

export interface AdminNodeConfigurationSync {
  desired: NodeConfiguration | null;
  desired_revision: number;
  effective: NodeConfiguration | null;
  effective_revision: number;
  status: NodeConfigurationSyncStatus;
  error: string | null;
  node_token_configured: boolean;
  updated_at: string | null;
  applied_at: string | null;
}

export interface NodeConfiguration {
  node: {
    id: string;
    region: string;
    control_api: string;
    work_root: string;
    capabilities: { build: boolean; serve: boolean };
  };
  build: {
    concurrency: number;
    command_timeout_seconds: number;
    retain_workspace_on_failure: boolean;
  };
  serve: {
    host: string;
    port: number;
    public_base_url: string;
    metadata_cache_ttl_seconds: number;
    artifact_cache_root: string;
    tls?: { enabled: boolean; port: number };
    capacity: AdminNodeResources;
    ssr: { idle_stop_seconds: number; startup_timeout_seconds: number };
  };
  runtime: {
    backend: "docker-socket" | "podman-socket";
    socket: string;
    default_build_image: string;
    default_serve_image: string;
    network: string;
    resources: { cpu_limit: number; memory_mb: number };
  };
  security: {
    gateway_authentication: "token" | "none";
    private_repository_targets: Array<{ host: string; ip: string; port: number }>;
  };
  development: { verbose_build_log: boolean };
  log: { level: string; format: "pretty" | "json" };
}

export interface AdminNodeResources {
  cpu_millicores: number;
  memory_mb: number;
  disk_mb: number;
  max_deployments: number;
}

export interface AdminNodeUsage {
  cpu_millicores: number;
  memory_mb: number;
  disk_mb: number;
  deployments: number;
}

export type LocalProcessState = "stopped" | "running" | "backoff" | "failed";

export interface AdminLocalProcess {
  state: LocalProcessState;
  pid: number | null;
  started_at: string | null;
  restart_count: number;
  last_exit_code: number | null;
  last_exit_at: string | null;
  message: string | null;
}

export interface AdminLocalProcessInfo {
  auto_start: boolean;
  managed: boolean;
  process: AdminLocalProcess;
}

export interface AdminQuotaLimit {
  dimension: string;
  limit_value: number;
  period: "none" | "monthly";
}

export interface AdminQuotaPlan {
  id: string;
  code: string;
  name: string;
  description: string | null;
  is_default: boolean;
  enabled: boolean;
  limits: AdminQuotaLimit[];
}

export type HostSourceKind = "wildcard" | "dns_provider" | "manual";

export interface AdminHostSource {
  id: string;
  kind: HostSourceKind;
  label: string;
  base_domain: string;
  region: string;
  enabled: boolean;
  allows_auto_assign: boolean;
  is_default: boolean;
  provider: string | null;
  config_keys: string[];
  created_at: string;
}

export interface AdminRegionalIngress {
  id: string;
  region: string;
  hostname: string;
  enabled: boolean;
  health_check_path: string;
  health_check_interval_seconds: number;
  origin_host_preservation: boolean;
  dns_status: "pending" | "resolved" | "unresolved" | "error";
  dns_checked_at: string | null;
  dns_error: string | null;
  node_statuses: Array<{
    node_id: string;
    tls_ready: boolean;
    health_status: string;
    checked_at: string | null;
  }>;
  healthy_nodes: Array<{ node_id: string; base_url: string; priority: number }>;
  created_at: string;
  updated_at: string;
}

export interface AdminUser {
  id: string;
  email: string;
  display_name: string | null;
  status: "active" | "disabled";
  platform_role: "user" | "admin";
  email_verified: boolean;
  last_login_at: string | null;
  created_at: string;
}

export interface AdminBatchItemResult {
  id: string;
  success: boolean;
  code?: number;
  message?: string;
}

export interface AdminUserFilters {
  q?: string;
  status?: AdminUser["status"];
  role?: AdminUser["platform_role"];
}

export type AdminCodeStatus = "available" | "used" | "expired" | "revoked";

export interface AdminCodeUser {
  id: string;
  email: string;
  display_name: string | null;
}

export interface AdminCode {
  id: string;
  code: string;
  scope: string;
  status: AdminCodeStatus;
  expires_at: string | null;
  used_at: string | null;
  used_by: AdminCodeUser | null;
  revoked_at: string | null;
  created_at: string;
}

export interface AdminGeneratedCode {
  id: string;
  code: string;
  scope: string;
  expires_at: string | null;
  created_at: string;
}

export interface AdminCodesPage {
  scopes: string[];
  codes: AdminCode[];
  pagination: {
    page: number;
    per_page: number;
    total: number;
    total_pages: number;
  };
}

export interface AdminRegistrationEmailCreator {
  id: string;
  email?: string | null;
  display_name?: string | null;
}

export interface AdminRegistrationEmail {
  id: string;
  email: string;
  created_at: string;
  created_by: AdminRegistrationEmailCreator | null;
}

export type AdminMfaEnforcement = "none" | "platform_admins" | "all_users";

export interface AdminMfaPolicy {
  allowed_factors: Array<"totp" | "email">;
  enforcement: AdminMfaEnforcement;
  minimum_factors: number;
  required_factors: Array<"totp" | "email">;
}

export interface AdminUserMfaPolicy {
  inherit_platform: boolean;
  minimum_factors: number;
  required_factors: Array<"totp" | "email">;
}

export interface AdminUserMfaResponse {
  factors: AdminMfaFactor[];
  policy: AdminUserMfaPolicy;
  allowed_factors: Array<"totp" | "email">;
  effective_requirements: {
    minimum_factors: number;
    required_factors: Array<"totp" | "email">;
  };
}

export interface AdminPasswordPolicy {
  min_length: number;
  max_length: number;
  require_lowercase: boolean;
  require_uppercase: boolean;
  require_number: boolean;
  require_symbol: boolean;
  history_count: number;
}

export interface AdminMfaFactor {
  id: string;
  kind: "totp" | "email";
  label: string | null;
  verified: boolean;
  verified_at: string | null;
  last_used_at: string | null;
  created_at: string;
}

export interface AdminIdentityProvider {
  id: string;
  slug: string;
  kind: "oidc" | "github";
  name: string;
  enabled: boolean;
  client_id: string;
  client_secret_configured: boolean;
  issuer_url: string | null;
  authorization_url: string;
  token_url: string;
  userinfo_url: string | null;
  jwks_url: string | null;
  scopes: string[];
  created_at: string;
  updated_at: string;
}

export interface AdminTeamGroupRef {
  id: string;
  code: string;
  name: string;
}

export interface AdminTeam {
  id: string;
  slug: string;
  name: string;
  kind: "personal" | "team";
  group: AdminTeamGroupRef | null;
  explicit_quota_plan_id: string | null;
  member_count: number;
  created_at: string;
}

export interface AdminTeamFilters {
  q?: string;
  kind?: AdminTeam["kind"];
  group_id?: string;
  quota_plan_id?: string;
}

export interface AdminTeamDetail {
  team: AdminTeam;
  members: {
    user_id: string;
    email: string;
    display_name: string | null;
    role: string;
    joined_at: string;
  }[];
  quota_plan: { id: string; code: string; name: string; source: string };
  project_count: number;
}

export interface AdminTeamGroup {
  id: string;
  code: string;
  name: string;
  description: string | null;
  quota_plan_id: string | null;
  review_policy: {
    production: "auto" | "manual" | null;
    preview: "auto" | "manual" | null;
    domain: "auto" | "manual" | null;
  };
  is_default: boolean;
  team_count?: number;
  created_at: string;
}

export interface QuotaLimitInput {
  dimension: string;
  limit_value: number | null;
}

export interface AdminProjectTeamRef {
  id: string;
  slug: string;
  name: string;
}

export interface AdminDeploymentSummary {
  id: string;
  environment: "production" | "preview";
  build_status: BuildStatus;
  release_status: ReleaseStatus;
  created_at: string;
}

export interface AdminProject {
  id: string;
  slug: string;
  name: string;
  runtime: string;
  repository_url: string | null;
  team: AdminProjectTeamRef | null;
  latest_deployment: AdminDeploymentSummary | null;
  status: "active" | "archived" | "deleted";
  archived_at: string | null;
  deleted_at: string | null;
  created_at: string;
}

export interface AdminProjectFilters {
  q?: string;
  status?: AdminProject["status"];
}

export interface AdminProjectDetail {
  id: string;
  uuid: string;
  slug: string;
  name: string;
  runtime: string;
  repository_url: string | null;
  default_branch: string | null;
  install_command: string | null;
  build_command: string | null;
  output_directory: string | null;
  source_config: Record<string, unknown>;
  build_config: Record<string, unknown>;
  status: "active" | "archived" | "deleted";
  archived_at: string | null;
  deleted_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface AdminGovernanceDeployment {
  id: string;
  project_id: string;
  environment: "production" | "preview";
  build_status: BuildStatus;
  serve_status: ServeStatus;
  release_status: ReleaseStatus;
  release_pending: boolean;
  preview_host: string | null;
  source_repository_url: string | null;
  source_branch: string | null;
  commit_hash: string | null;
  commit_message: string | null;
  build_stage: string | null;
  failure_code: string | null;
  failure_message: string | null;
  serve_failure_code: string | null;
  serve_failure_message: string | null;
  claimed_at: string | null;
  build_started_at: string | null;
  build_finished_at: string | null;
  serve_started_at: string | null;
  serve_finished_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface AdminProjectDomain {
  id: string;
  project_id: string;
  host: string;
  kind: "platform" | "custom";
  environment: "production" | "preview" | "all";
  status: "pending" | "active" | "failed" | "disabled";
  review_status: "not_required" | "pending" | "approved" | "rejected";
  failure_reason: string | null;
  is_primary: boolean;
  host_source_id: string | null;
  reviewed_by_user_id: string | null;
  reviewed_at: string | null;
  review_reason: string | null;
  created_at: string;
  updated_at: string;
}

export interface AdminDeploymentGovernanceResult {
  deployment: Pick<
    AdminGovernanceDeployment,
    "id" | "project_id" | "release_status" | "serve_status"
  >;
  reason: string | null;
}

export interface AdminDomainGovernanceResult {
  domain: Pick<
    AdminProjectDomain,
    | "id"
    | "project_id"
    | "host"
    | "status"
    | "review_status"
    | "reviewed_by_user_id"
    | "reviewed_at"
    | "review_reason"
    | "is_primary"
  >;
  reason: string | null;
}

export interface AdminReview {
  id: string;
  requested_at: string;
  deployment: {
    id: string;
    environment: "production" | "preview";
    build_status: BuildStatus;
    serve_status: ServeStatus;
    serve_was_ready: boolean;
    release_status: ReleaseStatus;
    source_branch: string | null;
    commit_hash: string | null;
    commit_message: string | null;
    preview_host: string | null;
    created_at: string;
  };
  project: { id: string; name: string; slug: string };
  team: AdminProjectTeamRef | null;
  triggered_by: { id: string; email: string; display_name: string | null } | null;
}

export interface AdminSettings {
  site: {
    name: string | null;
    logo_url: string | null;
    url: string | null;
    public_base_url: string | null;
  };
  signup: { policy: "open" | "invite_only" | "closed" };
  review: { production: "auto" | "manual"; preview: "auto" | "manual" };
  domain_review: { default: "auto" | "manual" };
  server: { host: string; port: number };
  database: { url_configured: boolean };
  redis: { backend: "moka" | "redis"; url_configured: boolean };
  secrets: { secret_key_configured: boolean; git_credentials_configured: boolean };
  mail: {
    mode: "none" | "local" | "smtp";
    from_address: string;
    from_name: string;
    sendmail_command: string;
    smtp_host: string;
    smtp_port: number;
    smtp_security: "none" | "starttls" | "tls";
    smtp_username: string;
    smtp_password_configured: boolean;
  };
  authentication: {
    password_policy: AdminPasswordPolicy;
    registration_email_verification: boolean;
    mfa_policy: AdminMfaPolicy;
  };
  session: {
    cookie_secure: boolean;
    idle_ttl_seconds: number;
    session_ttl_seconds: number;
  };
  audit: { retention_days: number };
  node_manager: {
    auto_start_local_node: boolean;
    local_node_binary: string;
    local_node_config: string;
    restart_on_exit: boolean;
  };
  migration: { auto_migrate: boolean };
  log: { level: string; format: "pretty" | "json" };
  restart_required_sections: Array<"server" | "redis" | "node_manager" | "migration" | "log">;
}

export type AdminStorageMigrationStatus = "pending" | "running" | "succeeded" | "failed";

export interface AdminStorageMigration {
  id: string;
  status: AdminStorageMigrationStatus;
  source: StorageConfiguration;
  target: StorageConfiguration;
  copied_objects: number;
  copied_bytes: number;
  total_objects: number | null;
  total_bytes: number | null;
  last_error: string | null;
  created_at: number;
  started_at: number | null;
  finished_at: number | null;
}

export interface AdminStorageState {
  storage: StorageConfiguration;
  maintenance: boolean;
  migration: AdminStorageMigration | null;
}

export type AdminAnnouncement = Announcement;

export interface AdminAnnouncementsPage {
  announcements: AdminAnnouncement[];
  pagination: {
    page: number;
    per_page: number;
    total: number;
    total_pages: number;
  };
}
