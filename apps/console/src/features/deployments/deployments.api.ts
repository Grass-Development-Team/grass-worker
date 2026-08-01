import { request } from "@/lib/api";

export type BuildStatus =
  | "pending"
  | "claimed"
  | "queued"
  | "building"
  | "ready"
  | "failed"
  | "canceled";

export type ReleaseStatus = "draft" | "pending_review" | "approved" | "rejected" | "active";
export type ServeStatus = "pending" | "syncing" | "ready" | "failed" | "retired";
export type DeploymentEnvironment = "production" | "preview";

export interface NodeRef {
  id: string;
  name: string;
}

export interface ServeResources {
  cpu_millicores: number;
  memory_mb: number;
  disk_mb: number;
}

export interface NodeResources extends ServeResources {
  max_deployments: number;
}

export interface NodeUsage extends ServeResources {
  deployments: number;
}

export interface ServeNodeTarget {
  id: string;
  name: string;
  healthy: boolean;
  capacity: NodeResources;
  usage: NodeUsage;
  normal_available: boolean;
  schedulable: boolean;
  overflow_only: boolean;
  disk_available_mb: number;
  remaining_overflow_slots: number;
}

export interface Deployment {
  id: string;
  project_id: string;
  team_id: string;
  build_node: NodeRef | null;
  serve_node: NodeRef | null;
  environment: DeploymentEnvironment;
  runtime_kind: string;
  build_status: BuildStatus;
  serve_status: ServeStatus;
  release_status: ReleaseStatus;
  release_pending: boolean;
  pending_release_reason: "promote" | "rollback" | null;
  pending_release_requested_at: string | null;
  serve_resources: ServeResources;
  overcommitted: boolean;
  build_stage: string | null;
  source: {
    repository_url: string | null;
    branch: string | null;
    commit_hash: string | null;
    commit_message: string | null;
  };
  triggered_by: { id: string; email: string; display_name: string | null } | null;
  failure_code: string | null;
  failure_message: string | null;
  serve_failure_code: string | null;
  serve_failure_message: string | null;
  duration_seconds: number | null;
  claimed_at: string | null;
  build_started_at: string | null;
  build_finished_at: string | null;
  serve_started_at: string | null;
  serve_finished_at: string | null;
  created_at: string;
  preview_url: string | null;
  production_url: string | null;
}

export interface DeploymentEvent {
  id: string;
  kind: "system" | "build" | "serve" | "release" | "review" | "host";
  message: string;
  metadata: Record<string, unknown>;
  created_at: string;
}

export interface DeploymentArtifact {
  id: string;
  kind: "grass_output" | "build_log" | "static_site";
  storage_path: string;
  checksum_sha256: string | null;
  size_bytes: number | null;
  manifest: Record<string, unknown>;
  created_at: string;
}

export interface DeploymentReview {
  id: string;
  status: "pending" | "approved" | "rejected";
  reviewer_user_id: string | null;
  reason: string | null;
  requested_at: string;
  reviewed_at: string | null;
}

export interface DeploymentDetail {
  deployment: Deployment;
  events: DeploymentEvent[];
  artifacts: DeploymentArtifact[];
  reviews: DeploymentReview[];
  review_required: boolean;
  was_active: boolean;
}

export interface BuildLogLine {
  seq: number;
  stage: string;
  line: string;
  timestamp_ms: number;
}

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
    input: { environment: DeploymentEnvironment; branch?: string; serve_node_id?: string },
  ) =>
    request<{ deployment: Deployment }>(`/api/v1/projects/${projectId}/deployments`, {
      method: "POST",
      body: JSON.stringify(input),
    }),

  serveNodes: (projectId: string) =>
    request<{ serve_nodes: ServeNodeTarget[] }>(`/api/v1/projects/${projectId}/serve-nodes`),

  detail: (projectId: string, deploymentId: string) =>
    request<DeploymentDetail>(`/api/v1/projects/${projectId}/deployments/${deploymentId}`),

  buildLog: (projectId: string, deploymentId: string, afterSeq: number) =>
    request<{ lines: BuildLogLine[]; last_seq: number; build_status: BuildStatus }>(
      `/api/v1/projects/${projectId}/deployments/${deploymentId}/build-log?after_seq=${afterSeq}`,
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
  const base = (import.meta.env.VITE_API_BASE_URL ?? "").trim().replace(/\/+$/, "");
  const path = `/api/v1/projects/${projectId}/deployments/${deploymentId}/logs/ws`;
  if (base.startsWith("http")) {
    return `${base.replace(/^http/, "ws")}${path}`;
  }
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${window.location.host}${path}`;
}

export function isBuildRunning(status: BuildStatus): boolean {
  return ["pending", "claimed", "queued", "building"].includes(status);
}

export function deploymentRefetchInterval(
  deployment:
    | Pick<Deployment, "build_status" | "serve_status" | "release_pending">
    | null
    | undefined,
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
