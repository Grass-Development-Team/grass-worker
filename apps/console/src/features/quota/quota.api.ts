import { request } from "@/lib/api";

export type QuotaPeriod = "none" | "monthly";

export interface QuotaPlanSummary {
  id: string;
  code: string;
  name: string;
  source: "explicit" | "group" | "default";
}

export interface QuotaLimitEntry {
  dimension: string;
  limit: number | null;
  period: QuotaPeriod;
}

export interface QuotaUsageEntry {
  dimension: string;
  limit: number | null;
  used: number;
  period: QuotaPeriod;
}

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
  "deployments.monthly": "Deployments this month",
  "build_minutes.monthly": "Build minutes this month",
  build_timeout_seconds: "Build timeout (seconds)",
  storage_mb: "Artifact storage (MB)",
  artifact_max_mb: "Max artifact size (MB)",
  concurrent_builds: "Concurrent builds",
  ssr_processes: "SSR processes (reserved)",
  "ssr_hours.monthly": "SSR hours this month (reserved)",
};

export function dimensionLabel(dimension: string): string {
  return DIMENSION_LABELS[dimension] ?? dimension;
}
