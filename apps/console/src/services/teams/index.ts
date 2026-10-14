import { request } from "@/infra/api/index";
import type {
  CreateSourceCredentialInput,
  InvitationCandidate,
  InvitationPreflight,
  ManagedTeamRole,
  SourceCredential,
  SourceCredentialSecretInput,
  SshHostKey,
  Team,
  TeamDetail,
  TeamInvitation,
  TeamMember,
} from "./model";
export type * from "./model";

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

  uploadAvatar: (teamId: string, png: Blob) =>
    request<{ team: Team }>(
      `/api/v1/teams/${teamId}/avatar`,
      withCredentials({
        method: "PUT",
        headers: { "Content-Type": "image/png" },
        body: png,
      }),
    ),

  deleteAvatar: (teamId: string) =>
    request<{ team: Team }>(
      `/api/v1/teams/${teamId}/avatar`,
      withCredentials({ method: "DELETE" }),
    ),

  listMembers: (teamId: string) =>
    request<{ members: TeamMember[] }>(`/api/v1/teams/${teamId}/members`, withCredentials()),

  invitationCandidates: (teamId: string, query: string) =>
    request<{ candidates: InvitationCandidate[] }>(
      `/api/v1/teams/${teamId}/invitation-candidates?${new URLSearchParams({ q: query })}`,
      withCredentials(),
    ),

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

  preflightInvitation: (token: string) =>
    request<InvitationPreflight>(
      `/api/v1/team-invitations/preflight?${new URLSearchParams({ token })}`,
      withCredentials(),
    ),

  listSourceCredentials: (teamId: string) =>
    request<{ credentials: SourceCredential[] }>(
      `/api/v1/teams/${teamId}/source-credentials`,
      withCredentials(),
    ),

  createSourceCredential: (teamId: string, input: CreateSourceCredentialInput) =>
    request<{ credential: SourceCredential }>(
      `/api/v1/teams/${teamId}/source-credentials`,
      withCredentials({ method: "POST", body: JSON.stringify(input) }),
    ),

  rotateSourceCredential: (
    teamId: string,
    credentialId: string,
    input: SourceCredentialSecretInput,
  ) =>
    request<{ credential: SourceCredential }>(
      `/api/v1/teams/${teamId}/source-credentials/${credentialId}/rotate`,
      withCredentials({ method: "POST", body: JSON.stringify(input) }),
    ),

  revokeSourceCredential: (teamId: string, credentialId: string) =>
    request<{ credential: SourceCredential }>(
      `/api/v1/teams/${teamId}/source-credentials/${credentialId}/revoke`,
      withCredentials({ method: "POST" }),
    ),

  listSshHostKeys: (teamId: string) =>
    request<{ host_keys: SshHostKey[] }>(
      `/api/v1/teams/${teamId}/ssh-host-keys`,
      withCredentials(),
    ),

  approveSshHostKey: (teamId: string, keyId: string) =>
    request<{ host_key: SshHostKey }>(
      `/api/v1/teams/${teamId}/ssh-host-keys/${keyId}/approve`,
      withCredentials({ method: "POST" }),
    ),

  rejectSshHostKey: (teamId: string, keyId: string) =>
    request<{ host_key: SshHostKey }>(
      `/api/v1/teams/${teamId}/ssh-host-keys/${keyId}/reject`,
      withCredentials({ method: "POST" }),
    ),
};
