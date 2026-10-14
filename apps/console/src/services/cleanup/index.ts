import { request } from "@/infra/api/index";
import type {
  AuditCleanupFilters,
  AuditCleanupPreview,
  BuildLogCleanupFilters,
  CleanupPreview,
  CleanupResult,
} from "./model";
export type * from "./model";

function query(filters: Record<string, string | number | undefined>) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(filters)) {
    if (value !== undefined && value !== "") params.set(key, String(value));
  }
  return params.size > 0 ? `?${params.toString()}` : "";
}

export const cleanupApi = {
  previewAudit: (filters: AuditCleanupFilters) =>
    request<AuditCleanupPreview>(`/api/v1/admin/cleanup/audit-events${query({ ...filters })}`),

  deleteAudit: (filters: AuditCleanupFilters) =>
    request<CleanupResult>("/api/v1/admin/cleanup/audit-events", {
      method: "DELETE",
      body: JSON.stringify(filters),
    }),

  previewBuildLogs: (filters: BuildLogCleanupFilters) =>
    request<CleanupPreview>(`/api/v1/admin/cleanup/build-logs${query({ ...filters })}`),

  deleteBuildLogs: (filters: BuildLogCleanupFilters) =>
    request<CleanupResult>("/api/v1/admin/cleanup/build-logs", {
      method: "DELETE",
      body: JSON.stringify(filters),
    }),
};
