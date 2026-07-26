import { request } from "@/lib/api";

export interface AuditEvent {
  id: string;
  actor_user_id: string | null;
  team_id: string | null;
  action: string;
  target_type: string;
  target_id: string | null;
  result: "success" | "failure" | "denied";
  reason: string | null;
  metadata: Record<string, unknown>;
  created_at: string;
}

export const auditApi = {
  listAdmin: (filters?: { action?: string; limit?: number }) => {
    const params = new URLSearchParams();
    if (filters?.action) params.set("action", filters.action);
    if (filters?.limit) params.set("limit", String(filters.limit));
    const query = params.size > 0 ? `?${params.toString()}` : "";
    return request<{ events: AuditEvent[] }>(`/api/v1/admin/audit-events${query}`);
  },

  listTeam: (teamId: string, filters?: { action?: string; limit?: number }) => {
    const params = new URLSearchParams();
    if (filters?.action) params.set("action", filters.action);
    if (filters?.limit) params.set("limit", String(filters.limit));
    const query = params.size > 0 ? `?${params.toString()}` : "";
    return request<{ events: AuditEvent[] }>(`/api/v1/teams/${teamId}/audit-events${query}`);
  },
};
