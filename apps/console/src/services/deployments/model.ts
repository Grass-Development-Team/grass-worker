export type BuildStatus =
  "pending" | "claimed" | "queued" | "building" | "ready" | "failed" | "canceled";

export type ReleaseStatus = "draft" | "pending_review" | "approved" | "rejected" | "active";

export type ServeStatus = "pending" | "syncing" | "ready" | "failed" | "retired";

export type DeploymentEnvironment = "production" | "preview";

export type ScreenshotStatus = "pending" | "ready" | "unavailable";

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
  region: string;
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
  region: string;
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
  screenshot_status: ScreenshotStatus;
  screenshot_url: string | null;
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
  kind: "grass_output" | "build_log" | "static_site" | "screenshot";
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
