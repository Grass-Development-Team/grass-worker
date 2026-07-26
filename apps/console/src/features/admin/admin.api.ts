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
};
