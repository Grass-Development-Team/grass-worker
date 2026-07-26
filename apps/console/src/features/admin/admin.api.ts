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

export const adminApi = {
  status: () => request<AdministrationStatus>("/api/v1/admin/status"),

  listNodes: () => request<{ nodes: AdminNode[] }>("/api/v1/admin/nodes"),

  createNode: (input: { name: string }) =>
    request<{ node: AdminNode; token: string }>("/api/v1/admin/nodes", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  rotateNodeToken: (nodeId: string) =>
    request<{ node_id: string; token: string }>(`/api/v1/admin/nodes/${nodeId}/rotate-token`, {
      method: "POST",
    }),
};
