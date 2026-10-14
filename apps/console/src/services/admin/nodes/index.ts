import { request } from "@/infra/api/index";
import type {
  AdminLocalProcess,
  AdminLocalProcessInfo,
  AdminNode,
  AdminNodeDeletionJob,
  AdminNodeDeletionPlan,
  NodeConfiguration,
} from "./model";
export const nodesApi = {
  listNodes: () =>
    request<{ nodes: AdminNode[]; local_process: AdminLocalProcessInfo }>("/api/v1/admin/nodes"),

  createNode: (input: { name: string; region?: string; start_local?: boolean }) =>
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
};
