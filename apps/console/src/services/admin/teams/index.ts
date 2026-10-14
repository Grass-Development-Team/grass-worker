import { request } from "@/infra/api/index";
import type { AdminBatchItemResult, AdminTeam, AdminTeamDetail, AdminTeamFilters } from "./model";
export const teamsApi = {
  listTeams: (filters: AdminTeamFilters = {}) => {
    const query = new URLSearchParams();
    if (filters.q?.trim()) query.set("q", filters.q.trim());
    if (filters.kind) query.set("kind", filters.kind);
    if (filters.group_id) query.set("group_id", filters.group_id);
    if (filters.quota_plan_id) query.set("quota_plan_id", filters.quota_plan_id);
    const suffix = query.size ? `?${query}` : "";
    return request<{ teams: AdminTeam[] }>(`/api/v1/admin/teams${suffix}`);
  },

  batchTeams: (
    input:
      | { action: "delete"; ids: string[] }
      | { action: "assign_group"; ids: string[]; group_id: string }
      | { action: "assign_quota_plan"; ids: string[]; plan_id: string | null },
  ) =>
    request<{ results: AdminBatchItemResult[] }>("/api/v1/admin/teams/batch", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  createTeam: (input: { name: string; slug?: string; owner_user_id: string }) =>
    request<{ team: AdminTeam }>("/api/v1/admin/teams", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  teamDetail: (teamId: string) => request<AdminTeamDetail>(`/api/v1/admin/teams/${teamId}`),

  updateTeam: (teamId: string, input: { name: string }) =>
    request<{ team: AdminTeam }>(`/api/v1/admin/teams/${teamId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  deleteTeam: (teamId: string) =>
    request<{ deleted: true }>(`/api/v1/admin/teams/${teamId}`, { method: "DELETE" }),

  assignTeamGroup: (teamId: string, groupId: string) =>
    request<{ team_id: string; group_id: string }>(`/api/v1/admin/teams/${teamId}/group`, {
      method: "POST",
      body: JSON.stringify({ group_id: groupId }),
    }),

  setTeamQuotaPlan: (teamId: string, planId: string | null) =>
    request<{ team: { id: string } }>(`/api/v1/admin/teams/${teamId}/quota-plan`, {
      method: "POST",
      body: JSON.stringify({ plan_id: planId }),
    }),
};
