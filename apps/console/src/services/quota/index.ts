import { request } from "@/infra/api/index";
import type { QuotaLimitEntry, QuotaPlanSummary, QuotaUsageEntry } from "./model";
export type * from "./model";

export const quotaApi = {
  plan: (teamId: string) =>
    request<{ plan: QuotaPlanSummary; limits: QuotaLimitEntry[] }>(`/api/v1/teams/${teamId}/quota`),

  usage: (teamId: string) =>
    request<{ plan: QuotaPlanSummary; usage: QuotaUsageEntry[] }>(
      `/api/v1/teams/${teamId}/quota/usage`,
    ),
};

export const DIMENSION_LABELS: Record<string, string> = {
  projects: "Projects",
  "projects.static": "Static projects",
  "projects.ssr": "SSR projects",
  members: "Team members",
  hosts: "Bound hosts",
  "deployments.monthly": "Deployments",
  "build_minutes.monthly": "Build minutes",
  build_timeout_seconds: "Build timeout (seconds)",
  storage_mb: "Artifact storage (MB)",
  artifact_max_mb: "Max artifact size (MB)",
  concurrent_builds: "Concurrent builds",
  ssr_processes: "SSR processes (reserved)",
  "ssr_hours.monthly": "SSR hours (reserved)",
};

export function dimensionLabel(dimension: string): string {
  return DIMENSION_LABELS[dimension] ?? dimension;
}
