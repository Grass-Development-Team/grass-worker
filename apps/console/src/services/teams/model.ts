export type TeamKind = "personal" | "team";

export type TeamRole = "owner" | "admin" | "member" | "viewer";

export type ManagedTeamRole = Exclude<TeamRole, "owner">;

export interface Team {
  id: string;
  slug: string;
  name: string;
  avatar_url: string | null;
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
