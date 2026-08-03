import type { AuditFilters } from "@/features/audit/audit.api";
import { request } from "@/lib/api";

export interface BuildLogCleanupFilters {
  deployment_id?: string;
  project_id?: string;
  team_id?: string;
  triggered_by_user_id?: string;
  from?: number;
  to?: number;
}

export interface CleanupPreview {
  matched: number;
  deletable: number;
  skipped: number;
}

export interface CleanupResult {
  deleted: number;
  skipped: number;
  failed?: number;
}

function query(filters: Record<string, string | number | undefined>) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(filters)) {
    if (value !== undefined && value !== "") params.set(key, String(value));
  }
  return params.size > 0 ? `?${params.toString()}` : "";
}

export const cleanupApi = {
  previewAudit: (filters: AuditFilters) =>
    request<CleanupPreview>(`/api/v1/admin/cleanup/audit-events${query(filters)}`),

  deleteAudit: (filters: AuditFilters) =>
    request<CleanupResult>("/api/v1/admin/cleanup/audit-events", {
      method: "DELETE",
      body: JSON.stringify(filters),
    }),

  previewBuildLogs: (filters: BuildLogCleanupFilters) =>
    request<CleanupPreview>(`/api/v1/admin/cleanup/build-logs${query(filters)}`),

  deleteBuildLogs: (filters: BuildLogCleanupFilters) =>
    request<CleanupResult>("/api/v1/admin/cleanup/build-logs", {
      method: "DELETE",
      body: JSON.stringify(filters),
    }),
};
