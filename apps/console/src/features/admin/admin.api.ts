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
  last_heartbeat_at: string | null;
  created_at: string;
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
  is_default: boolean;
  team_count?: number;
  created_at: string;
}

export interface QuotaLimitInput {
  dimension: string;
  limit_value: number | null;
}

export interface AdminSettings {
  site: { name: string | null; url: string | null; public_base_url: string | null };
  storage: { root: string };
  signup: { policy: "open" | "invite_only" | "closed" };
  review: { production: "auto" | "manual"; preview: "auto" | "manual" };
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
  }) =>
    request<{ source: AdminHostSource }>("/api/v1/admin/host-sources", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateHostSource: (
    sourceId: string,
    input: Partial<
      Pick<AdminHostSource, "label" | "enabled" | "allows_auto_assign" | "is_default">
    >,
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

  getSettings: () => request<AdminSettings>("/api/v1/admin/settings"),

  updateSettings: (input: {
    site_name?: string;
    site_url?: string;
    public_base_url?: string;
    storage_root?: string;
    signup_policy?: "open" | "invite_only" | "closed";
    review_production?: "auto" | "manual";
    review_preview?: "auto" | "manual";
  }) =>
    request<AdminSettings>("/api/v1/admin/settings", {
      method: "PATCH",
      body: JSON.stringify(input),
    }),
};
