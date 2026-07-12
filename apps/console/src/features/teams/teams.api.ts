import { request } from "@/lib/api";

export type TeamKind = "personal" | "team";
export type TeamRole = "owner" | "admin" | "member" | "viewer";
export type ManagedTeamRole = Exclude<TeamRole, "owner">;

export interface Team {
  id: string;
  slug: string;
  name: string;
  kind: TeamKind;
  owner_user_id: string | null;
  group_id: string | null;
}

export interface TeamDetail extends Team {
  role: TeamRole;
}

export interface TeamMember {
  id: string;
  user_id: string;
  email: string;
  display_name: string | null;
  role: TeamRole;
  joined_at: string;
}

export interface TeamInvitation {
  id: string;
  team_id: string;
  email: string;
  role: ManagedTeamRole;
  status: "pending";
  expires_at: string;
  token: string;
}

const withCredentials = (options?: RequestInit): RequestInit => ({
  ...options,
  credentials: "include",
});

export const teamsApi = {
  list: () => request<{ teams: Team[] }>("/api/v1/teams", withCredentials()),

  get: (teamId: string) =>
    request<{ team: TeamDetail }>(`/api/v1/teams/${teamId}`, withCredentials()),

  create: (input: { name: string; slug: string }) =>
    request<{ team: Team }>(
      "/api/v1/teams",
      withCredentials({ method: "POST", body: JSON.stringify(input) }),
    ),

  update: (teamId: string, input: { name: string; slug: string }) =>
    request<{ team: Team }>(
      `/api/v1/teams/${teamId}`,
      withCredentials({ method: "PATCH", body: JSON.stringify(input) }),
    ),

  listMembers: (teamId: string) =>
    request<{ members: TeamMember[] }>(`/api/v1/teams/${teamId}/members`, withCredentials()),

  inviteMember: (teamId: string, input: { email: string; role: ManagedTeamRole }) =>
    request<{ invitation: TeamInvitation }>(
      `/api/v1/teams/${teamId}/invitations`,
      withCredentials({ method: "POST", body: JSON.stringify(input) }),
    ),

  updateMemberRole: (teamId: string, userId: string, role: ManagedTeamRole) =>
    request<{ member: Pick<TeamMember, "id" | "user_id" | "role"> }>(
      `/api/v1/teams/${teamId}/members/${userId}`,
      withCredentials({ method: "PATCH", body: JSON.stringify({ role }) }),
    ),

  removeMember: (teamId: string, userId: string) =>
    request<{ ok: true }>(
      `/api/v1/teams/${teamId}/members/${userId}`,
      withCredentials({ method: "DELETE" }),
    ),

  acceptInvitation: (token: string) =>
    request<{ member: Pick<TeamMember, "id" | "user_id" | "role"> & { team_id: string } }>(
      "/api/v1/team-invitations/accept",
      withCredentials({ method: "POST", body: JSON.stringify({ token }) }),
    ),
};
