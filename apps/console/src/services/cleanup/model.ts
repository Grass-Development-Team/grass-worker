import type { AuditEvent, AuditFilters, AuditPage } from "@/services/audit/index";

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

export interface AuditCleanupFilters extends AuditFilters {
  snapshot_before?: number;
}

export interface AuditCleanupPreview extends CleanupPreview {
  events: AuditEvent[];
  pagination: AuditPage["pagination"];
  snapshot_before: number;
}

export interface CleanupResult {
  deleted: number;
  skipped: number;
  failed?: number;
}
