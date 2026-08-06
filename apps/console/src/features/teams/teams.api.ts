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

export interface InvitationCandidate {
  kind: "user" | "email";
  user_id: string | null;
  email: string;
  display_name: string | null;
}

export interface InvitationPreflight {
  team: Pick<Team, "id" | "name">;
  role: ManagedTeamRole;
  status: "pending" | "expired" | "accepted" | "revoked" | "email_mismatch";
  expires_at: string;
  email_matches_current_user: boolean | null;
  can_accept: boolean;
}

export interface SourceCredential {
  id: string;
  team_id: string;
  name: string;
  kind: "https" | "ssh";
  host: string;
  port: number;
  username: string | null;
  current_version_id: string | null;
  revoked_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface SourceCredentialSecretInput {
  username: string;
  secret?: string;
  private_key?: string;
  passphrase?: string;
}

export interface CreateSourceCredentialInput extends SourceCredentialSecretInput {
  name: string;
  repository_url: string;
}

export interface SshHostKey {
  id: string;
  host: string;
  port: number;
  key_type: string;
  fingerprint_sha256: string;
  status: "pending" | "approved" | "rejected" | "superseded";
  approved_at: string | null;
  last_seen_at: string;
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
