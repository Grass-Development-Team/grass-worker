import type {
  BuildStatus,
  ReleaseStatus,
  ServeStatus,
} from "@/features/deployments/deployments.api";
import { request } from "@/lib/api";

export interface AdministrationStatus {
  service: string;
  mode: "ready";
  version: string;
}

export type NodeStatus = "pending" | "active" | "draining" | "offline" | "disabled";

export interface AdminNode {
  id: string;
  name: string;
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
  | "queued"
  | "migrating"
  | "draining"
  | "deleting"
  | "failed"
  | "completed";

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

export interface UpdateNodeCapacityInput {
  capacity_cpu_millicores: number;
  capacity_memory_mb: number;
  capacity_disk_mb: number;
  max_deployments: number;
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
  enabled: boolean;
  allows_auto_assign: boolean;
  is_default: boolean;
  provider: string | null;
  config_keys: string[];
  created_at: string;
}

export interface AdminUser {
  id: string;
  email: string;
  display_name: string | null;
  status: "active" | "disabled";
  platform_role: "user" | "admin";
  last_login_at: string | null;
  created_at: string;
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
  archived_at: string | null;
  created_at: string;
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
  site: { name: string | null; url: string | null; public_base_url: string | null };
  storage: { root: string };
  signup: { policy: "open" | "invite_only" | "closed" };
  review: { production: "auto" | "manual"; preview: "auto" | "manual" };
  server: { host: string; port: number };
  database: { url_configured: boolean };
  redis: { backend: "moka" | "redis"; url_configured: boolean };
  secrets: { secret_key_configured: boolean; git_credentials_configured: boolean };
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

export const adminApi = {
  status: () => request<AdministrationStatus>("/api/v1/admin/status"),

  listQuotaPlans: () => request<{ plans: AdminQuotaPlan[] }>("/api/v1/admin/quota-plans"),

  listHostSources: () => request<{ sources: AdminHostSource[] }>("/api/v1/admin/host-sources"),

  createHostSource: (input: {
    label: string;
    kind: HostSourceKind;
    base_domain: string;
    is_default?: boolean;
    provider?: string;
    config?: Record<string, unknown>;
  }) =>
    request<{ source: AdminHostSource }>("/api/v1/admin/host-sources", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateHostSource: (
    sourceId: string,
    input: Partial<
      Pick<AdminHostSource, "label" | "enabled" | "allows_auto_assign" | "is_default">
    > & {
      provider?: string;
      /** Shallow-merged server side; a null value deletes the key. */
      config?: Record<string, unknown>;
    },
  ) =>
    request<{ source: AdminHostSource }>(`/api/v1/admin/host-sources/${sourceId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  removeHostSource: (sourceId: string) =>
    request<{ ok: true }>(`/api/v1/admin/host-sources/${sourceId}`, { method: "DELETE" }),

  listNodes: () =>
    request<{ nodes: AdminNode[]; local_process: AdminLocalProcessInfo }>("/api/v1/admin/nodes"),

  createNode: (input: { name: string; start_local?: boolean }) =>
    request<{
      node: AdminNode;
      token: string;
      local_process: AdminLocalProcess | null;
      warnings: string[];
    }>("/api/v1/admin/nodes", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  localNodeProcess: (action: "start" | "stop" | "restart") =>
    request<AdminLocalProcessInfo>("/api/v1/admin/nodes/local-process", {
      method: "POST",
      body: JSON.stringify({ action }),
    }),

  rotateNodeToken: (nodeId: string) =>
    request<{ node_id: string; token: string }>(`/api/v1/admin/nodes/${nodeId}/rotate-token`, {
      method: "POST",
    }),

  updateNodeCapacity: (nodeId: string, input: UpdateNodeCapacityInput) =>
    request<{ node: AdminNode }>(`/api/v1/admin/nodes/${nodeId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  updateNodeConfiguration: (nodeId: string, input: NodeConfiguration) =>
    request<{ node: AdminNode }>(`/api/v1/admin/nodes/${nodeId}/configuration`, {
      method: "PUT",
      body: JSON.stringify(input),
    }),

  nodeDeletionPlan: (nodeId: string) =>
    request<AdminNodeDeletionPlan>(`/api/v1/admin/nodes/${nodeId}/deletion-plan`),

  queueNodeDeletion: (nodeId: string, input: { target_node_id: string | null }) =>
    request<{ job: AdminNodeDeletionJob }>(`/api/v1/admin/nodes/${nodeId}/deletion`, {
      method: "POST",
      body: JSON.stringify(input),
    }),

  listUsers: (q?: string) =>
    request<{ users: AdminUser[] }>(
      `/api/v1/admin/users${q?.trim() ? `?q=${encodeURIComponent(q.trim())}` : ""}`,
    ),

  createUser: (input: {
    email: string;
    display_name?: string;
    platform_role?: "user" | "admin";
    password?: string;
  }) =>
    request<{ user: AdminUser; password: string | null }>("/api/v1/admin/users", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateUser: (
    userId: string,
    input: {
      display_name?: string | null;
      status?: "active" | "disabled";
      platform_role?: "user" | "admin";
    },
  ) =>
    request<{ user: AdminUser }>(`/api/v1/admin/users/${userId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  resetUserPassword: (userId: string, password?: string) =>
    request<{ user_id: string; password: string | null }>(
      `/api/v1/admin/users/${userId}/reset-password`,
      {
        method: "POST",
        body: JSON.stringify(password ? { password } : {}),
      },
    ),

  listTeams: (q?: string) =>
    request<{ teams: AdminTeam[] }>(
      `/api/v1/admin/teams${q?.trim() ? `?q=${encodeURIComponent(q.trim())}` : ""}`,
    ),

  createTeam: (input: { name: string; slug?: string; owner_user_id: string }) =>
    request<{ team: AdminTeam }>("/api/v1/admin/teams", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  teamDetail: (teamId: string) => request<AdminTeamDetail>(`/api/v1/admin/teams/${teamId}`),

  updateTeam: (teamId: string, input: { name: string }) =>
    request<{ team: AdminTeam }>(`/api/v1/admin/teams/${teamId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  deleteTeam: (teamId: string) =>
    request<{ deleted: true }>(`/api/v1/admin/teams/${teamId}`, { method: "DELETE" }),

  listTeamGroups: () => request<{ groups: AdminTeamGroup[] }>("/api/v1/admin/team-groups"),

  assignTeamGroup: (teamId: string, groupId: string) =>
    request<{ team_id: string; group_id: string }>(`/api/v1/admin/teams/${teamId}/group`, {
      method: "POST",
      body: JSON.stringify({ group_id: groupId }),
    }),

  createQuotaPlan: (input: {
    code: string;
    name: string;
    description?: string;
    limits: QuotaLimitInput[];
  }) =>
    request<{ plan: { id: string; code: string; name: string } }>("/api/v1/admin/quota-plans", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateQuotaPlan: (
    planId: string,
    input: {
      name?: string;
      description?: string;
      enabled?: boolean;
      is_default?: boolean;
      limits?: QuotaLimitInput[];
    },
  ) =>
    request<{ plan: { id: string } }>(`/api/v1/admin/quota-plans/${planId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  createTeamGroup: (input: {
    code: string;
    name: string;
    description?: string;
    quota_plan_id?: string;
    review_policy?: {
      production: "auto" | "manual" | null;
      preview: "auto" | "manual" | null;
    };
  }) =>
    request<{ group: AdminTeamGroup }>("/api/v1/admin/team-groups", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateTeamGroup: (
    groupId: string,
    input: {
      name?: string;
      description?: string;
      quota_plan_id?: string | null;
      review_policy?: {
        production: "auto" | "manual" | null;
        preview: "auto" | "manual" | null;
      };
      is_default?: boolean;
    },
  ) =>
    request<{ group: AdminTeamGroup }>(`/api/v1/admin/team-groups/${groupId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  deleteTeamGroup: (groupId: string) =>
    request<{ deleted: true }>(`/api/v1/admin/team-groups/${groupId}`, { method: "DELETE" }),

  setTeamQuotaPlan: (teamId: string, planId: string | null) =>
    request<{ team: { id: string } }>(`/api/v1/admin/teams/${teamId}/quota-plan`, {
      method: "POST",
      body: JSON.stringify({ plan_id: planId }),
    }),

  listProjects: (q?: string) =>
    request<{ projects: AdminProject[] }>(
      `/api/v1/admin/projects${q?.trim() ? `?q=${encodeURIComponent(q.trim())}` : ""}`,
    ),

  archiveProject: (projectId: string) =>
    request<{ project: AdminProject }>(`/api/v1/admin/projects/${projectId}/archive`, {
      method: "POST",
    }),

  unarchiveProject: (projectId: string) =>
    request<{ project: AdminProject }>(`/api/v1/admin/projects/${projectId}/unarchive`, {
      method: "POST",
    }),

  deleteProject: (projectId: string) =>
    request<{ deleted: true }>(`/api/v1/admin/projects/${projectId}/delete`, { method: "POST" }),

  listReviews: () => request<{ total: number; reviews: AdminReview[] }>("/api/v1/admin/reviews"),

  approveReview: (deploymentId: string, opts?: { reason?: string; promote?: boolean }) =>
    request<{
      deployment_id: string;
      release_status: string;
      promoted: boolean;
      release_pending: boolean;
    }>(`/api/v1/admin/deployments/${deploymentId}/review/approve`, {
      method: "POST",
      body: JSON.stringify(opts ?? {}),
    }),

  rejectReview: (deploymentId: string, reason?: string) =>
    request<{ deployment_id: string; release_status: string }>(
      `/api/v1/admin/deployments/${deploymentId}/review/reject`,
      { method: "POST", body: JSON.stringify(reason ? { reason } : {}) },
    ),

  getSettings: () => request<AdminSettings>("/api/v1/admin/settings"),

  updateSettings: (input: {
    site_name?: string;
    site_url?: string;
    public_base_url?: string;
    storage_root?: string;
    signup_policy?: "open" | "invite_only" | "closed";
    review_production?: "auto" | "manual";
    review_preview?: "auto" | "manual";
    server_host?: string;
    server_port?: number;
    redis_backend?: "moka" | "redis";
    session_cookie_secure?: boolean;
    session_idle_ttl_seconds?: number;
    session_ttl_seconds?: number;
    audit_retention_days?: number;
    node_manager_auto_start_local_node?: boolean;
    node_manager_local_node_binary?: string;
    node_manager_local_node_config?: string;
    node_manager_restart_on_exit?: boolean;
    migration_auto_migrate?: boolean;
    log_level?: string;
    log_format?: "pretty" | "json";
  }) =>
    request<AdminSettings>("/api/v1/admin/settings", {
      method: "PATCH",
      body: JSON.stringify(input),
    }),
};
