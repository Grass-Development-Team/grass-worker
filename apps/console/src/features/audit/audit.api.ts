import { request } from "@/lib/api";

export type AuditActorType = "anonymous" | "user" | "system" | "node";
export type AuditResult = "success" | "failure" | "denied";

export interface AuditEvent {
  id: string;
  actor_user_id: string | null;
  actor_node_id: string | null;
  actor_type: AuditActorType;
  team_id: string | null;
  visibility: "platform" | "team";
  action: string;
  target_type: string;
  target_id: string | null;
  result: AuditResult;
  reason: string | null;
  metadata: Record<string, unknown>;
  request_id: string | null;
  source_ip: string | null;
  user_agent: string | null;
  http_method: string | null;
  request_path: string | null;
  status_code: number | null;
  duration_ms: number | null;
  changes: Record<string, unknown>;
  created_at: string;
}

export interface AuditFilters {
  action?: string;
  actor_user_id?: string;
  actor_type?: AuditActorType;
  target_type?: string;
  target_id?: string;
  team_id?: string;
  result?: AuditResult;
  from?: number;
  to?: number;
  page?: number;
  per_page?: number;
}

export interface AuditPage {
  events: AuditEvent[];
  pagination: {
    page: number;
    per_page: number;
    total: number;
    total_pages: number;
  };
}

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
