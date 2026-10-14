import { request } from "@/infra/api/index";
import type { AdminTeamGroup } from "./model";
export const teamGroupsApi = {
  listTeamGroups: () => request<{ groups: AdminTeamGroup[] }>("/api/v1/admin/team-groups"),

  createTeamGroup: (input: {
    code: string;
    name: string;
    description?: string;
    quota_plan_id?: string;
    review_policy?: {
      production: "auto" | "manual" | null;
      preview: "auto" | "manual" | null;
      domain: "auto" | "manual" | null;
    };
  }) =>
    request<{ group: AdminTeamGroup }>("/api/v1/admin/team-groups", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  updateTeamGroup: (
    groupId: string,
    input: {
      name?: string;
      description?: string;
      quota_plan_id?: string | null;
      review_policy?: {
        production: "auto" | "manual" | null;
        preview: "auto" | "manual" | null;
        domain: "auto" | "manual" | null;
      };
      is_default?: boolean;
    },
  ) =>
    request<{ group: AdminTeamGroup }>(`/api/v1/admin/team-groups/${groupId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  deleteTeamGroup: (groupId: string) =>
    request<{ deleted: true }>(`/api/v1/admin/team-groups/${groupId}`, { method: "DELETE" }),
};
