import { request } from "@/infra/api/index";
import type { AuditFilters, AuditPage } from "./model";
export type * from "./model";

function query(filters?: AuditFilters) {
  const params = new URLSearchParams();
  if (!filters) return "";
  for (const [key, value] of Object.entries(filters)) {
    if (value !== undefined && value !== "") params.set(key, String(value));
  }
  return params.size > 0 ? `?${params.toString()}` : "";
}

export const auditApi = {
  listAdmin: (filters?: AuditFilters) =>
    request<AuditPage>(`/api/v1/admin/audit-events${query(filters)}`),

  listTeam: (teamId: string, filters?: AuditFilters) =>
    request<AuditPage>(`/api/v1/teams/${teamId}/audit-events${query(filters)}`),
};
