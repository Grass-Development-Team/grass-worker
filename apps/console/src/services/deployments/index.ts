import { apiUrl, request } from "@/infra/api/index";
import type {
  BuildLogLine,
  BuildStatus,
  Deployment,
  DeploymentDetail,
  DeploymentEnvironment,
  ServeNodeTarget,
} from "./model";
export type * from "./model";

export const deploymentsApi = {
  list: (projectId: string, filters?: { environment?: DeploymentEnvironment }) => {
    const params = new URLSearchParams();
    if (filters?.environment) params.set("environment", filters.environment);
    const query = params.size > 0 ? `?${params.toString()}` : "";
    return request<{ deployments: Deployment[] }>(
      `/api/v1/projects/${projectId}/deployments${query}`,
    );
  },

  create: (
    projectId: string,
    input: {
      environment: DeploymentEnvironment;
      branch?: string;
      serve_node_id?: string;
      region?: string;
    },
  ) =>
    request<{ deployment: Deployment }>(`/api/v1/projects/${projectId}/deployments`, {
      method: "POST",
      body: JSON.stringify(input),
    }),

  serveNodes: (projectId: string, region?: string) => {
    const query = region ? `?region=${encodeURIComponent(region)}` : "";
    return request<{ serve_nodes: ServeNodeTarget[] }>(
      `/api/v1/projects/${projectId}/serve-nodes${query}`,
    );
  },

  detail: (projectId: string, deploymentId: string) =>
    request<DeploymentDetail>(`/api/v1/projects/${projectId}/deployments/${deploymentId}`),

  buildLog: (projectId: string, deploymentId: string, afterSeq: number, signal?: AbortSignal) =>
    request<{ lines: BuildLogLine[]; last_seq: number; build_status: BuildStatus }>(
      `/api/v1/projects/${projectId}/deployments/${deploymentId}/build-log?after_seq=${afterSeq}`,
      { signal },
    ),

  cancel: (projectId: string, deploymentId: string) =>
    request<{ deployment: Deployment }>(
      `/api/v1/projects/${projectId}/deployments/${deploymentId}/cancel`,
      { method: "POST" },
    ),

  unpublish: (projectId: string, deploymentId: string) =>
    request<{ deployment: Deployment }>(
      `/api/v1/projects/${projectId}/deployments/${deploymentId}/unpublish`,
      { method: "POST" },
    ),

  retry: (projectId: string, deploymentId: string) =>
    request<{ deployment: Deployment }>(
      `/api/v1/projects/${projectId}/deployments/${deploymentId}/retry`,
      { method: "POST" },
    ),

  promote: (projectId: string, deploymentId: string) =>
    request<{ deployment: Deployment; release_pending: boolean }>(
      `/api/v1/projects/${projectId}/deployments/${deploymentId}/promote`,
      { method: "POST" },
    ),

  rollback: (projectId: string, deploymentId: string) =>
    request<{ deployment: Deployment; release_pending: boolean }>(
      `/api/v1/projects/${projectId}/deployments/${deploymentId}/rollback`,
      { method: "POST" },
    ),
};

export function logStreamUrl(projectId: string, deploymentId: string): string {
  const path = `/api/v1/projects/${projectId}/deployments/${deploymentId}/logs/ws`;
  const url = new URL(apiUrl(path), window.location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}

export function isBuildRunning(status: BuildStatus): boolean {
  return ["pending", "claimed", "queued", "building"].includes(status);
}

export function deploymentRefetchInterval(
  deployment:
    Pick<Deployment, "build_status" | "serve_status" | "release_pending"> | null | undefined,
): 4000 | false {
  if (!deployment) return false;
  if (isBuildRunning(deployment.build_status)) return 4000;
  if (deployment.release_pending) return 4000;
  return deployment.build_status === "ready" &&
    ["pending", "syncing"].includes(deployment.serve_status)
    ? 4000
    : false;
}

export function formatDuration(seconds: number | null): string {
  if (seconds === null) return "—";
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

export function shortCommit(hash: string | null): string {
  return hash ? hash.slice(0, 7) : "—";
}
