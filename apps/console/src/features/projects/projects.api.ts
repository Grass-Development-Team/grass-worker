import { request } from "@/lib/api";

type ProjectRuntime = "static" | "ssr" | "hybrid" | "serverless" | "edge";
export type HostStatus = "pending" | "active" | "failed" | "disabled";
type HostEnvironment = "production" | "preview" | "all";

export interface Project {
  id: string;
  team_id: string;
  slug: string;
  name: string;
  runtime: ProjectRuntime;
  repository_url: string | null;
  default_branch: string | null;
  install_command: string | null;
  build_command: string | null;
  output_directory: string | null;
  source_config: { root_directory?: string | null; framework_hint?: string | null };
  build_config: Record<string, unknown>;
  archived_at: string | null;
  deleted_at: string | null;
  created_at: string;
  updated_at: string;
}

interface HostAssignment {
  assigned: boolean;
  host?: string;
  status?: HostStatus;
  failure_reason?: string | null;
  reason?: string;
}

interface ProvisionEvent {
  id: string;
  status: "success" | "pending" | "failed";
  operation: string;
  error_message: string | null;
  created_at: string;
}

export interface ProjectHost {
  id: string;
  project_id: string;
  host: string;
  region?: string;
  kind: "platform" | "custom";
  environment: HostEnvironment;
  status: HostStatus;
  failure_reason: string | null;
  is_primary: boolean;
  host_source_id: string | null;
  serving?: boolean;
  created_at: string;
  provision_events?: ProvisionEvent[];
  ingress?: ProjectHostIngress | null;
  ownership_status?: "pending" | "verified" | "failed" | "not_required";
  ownership_checked_at?: string | null;
  ownership_error?: string | null;
  certificate?: DomainCertificate | null;
  connection_state?: string;
  onboarding?: {
    dns_status: string;
    dns_error: string | null;
    checked_at: string | null;
    next_check_at: string;
  } | null;
}

export interface DomainCertificate {
  status: "pending" | "issuing" | "active" | "expiring" | "failed" | "disabled";
  issuer: "letsencrypt" | "zerossl" | "manual";
  platform_issuer: "letsencrypt" | "zerossl";
  // Installation progress is included in domain list and connection-check responses.
  https_ready?: boolean;
  installed_nodes?: number;
  required_nodes?: number;
  challenge_method: "http01";
  auto_renew: boolean;
  issued_at: string | null;
  expires_at: string | null;
  error: string | null;
  retry_at: string | null;
  revision: string | null;
}

interface ProjectHostIngress {
  region: string;
  cname: { record_type: "CNAME"; name: string; target: string };
  txt: { record_type: "TXT"; name: string; value: string };
  origin_host_preservation: boolean;
  health_check: { path: string; interval_seconds: number };
  entrance_nodes: Array<{ node_id: string; base_url: string; priority: number }>;
  certificate: {
    enabled: boolean;
    issuer: "letsencrypt" | "zerossl" | "manual";
    auto_renew: boolean;
    status: "pending" | "issuing" | "active" | "expiring" | "failed" | "disabled";
    expires_at: string | null;
    error: string | null;
  };
}

export interface CreateProjectInput {
  team_id: string;
  name: string;
  slug: string;
  runtime: "static" | "ssr";
  repository_url?: string;
  default_branch?: string;
  root_directory?: string;
  install_command?: string;
  build_command?: string;
  output_directory?: string;
  framework_hint?: string;
}

export interface UpdateProjectInput {
  name?: string;
  repository_url?: string;
  default_branch?: string;
  install_command?: string;
  build_command?: string;
  output_directory?: string;
  root_directory?: string;
  framework_hint?: string;
}

interface BoundSourceCredential {
  id: string;
  name: string;
  kind: "https" | "ssh";
  host: string;
  port: number;
  username: string | null;
  revoked: boolean;
}

export const projectsApi = {
  list: (teamId: string) =>
    request<{ projects: Project[] }>(`/api/v1/projects?team_id=${encodeURIComponent(teamId)}`),

  create: (input: CreateProjectInput) =>
    request<{ project: Project; host_assignment: HostAssignment }>("/api/v1/projects", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  get: (projectId: string) =>
    request<{ project: Project; team: { id: string; slug: string; name: string }; role: string }>(
      `/api/v1/projects/${projectId}`,
    ),

  update: (projectId: string, input: UpdateProjectInput) =>
    request<{ project: Project }>(`/api/v1/projects/${projectId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),

  getSourceCredential: (projectId: string) =>
    request<{ credential: BoundSourceCredential | null }>(
      `/api/v1/projects/${projectId}/source-credential`,
    ),

  bindSourceCredential: (projectId: string, credentialId: string) =>
    request<{ credential_id: string }>(`/api/v1/projects/${projectId}/source-credential`, {
      method: "POST",
      body: JSON.stringify({ credential_id: credentialId }),
    }),

  unbindSourceCredential: (projectId: string) =>
    request<{ unbound: true }>(`/api/v1/projects/${projectId}/source-credential`, {
      method: "DELETE",
    }),

  archive: (projectId: string) =>
    request<{ project: Project }>(`/api/v1/projects/${projectId}/archive`, { method: "POST" }),

  unarchive: (projectId: string) =>
    request<{ project: Project }>(`/api/v1/projects/${projectId}/unarchive`, { method: "POST" }),

  softDelete: (projectId: string) =>
    request<{ project: Project }>(`/api/v1/projects/${projectId}/delete`, { method: "POST" }),

  listHosts: (projectId: string) =>
    request<{ hosts: ProjectHost[] }>(`/api/v1/projects/${projectId}/hosts`),

  createHost: (
    projectId: string,
    input: { host: string; region?: string; environment?: HostEnvironment },
  ) =>
    request<{ host: ProjectHost }>(`/api/v1/projects/${projectId}/hosts`, {
      method: "POST",
      body: JSON.stringify(input),
    }),

  removeHost: (projectId: string, hostId: string) =>
    request<{ ok: true }>(`/api/v1/projects/${projectId}/hosts/${hostId}`, { method: "DELETE" }),

  setPrimaryHost: (projectId: string, hostId: string) =>
    request<{ ok: true }>(`/api/v1/projects/${projectId}/hosts/${hostId}/primary`, {
      method: "POST",
    }),

  provisionHost: (projectId: string, hostId: string) =>
    request<{ host: ProjectHost }>(`/api/v1/projects/${projectId}/hosts/${hostId}/provision`, {
      method: "POST",
    }),

  verifyHost: (projectId: string, hostId: string) =>
    request<{ host: ProjectHost; verified: boolean }>(
      `/api/v1/projects/${projectId}/hosts/${hostId}/verify`,
      { method: "POST" },
    ),

  updateHostCertificate: (
    projectId: string,
    hostId: string,
    input: {
      challenge_method?: "http01" | "dns01";
      certificate_auto_renew?: boolean;
      certificate_issuer?: DomainCertificate["issuer"];
    },
  ) =>
    request<{ certificate: DomainCertificate }>(
      `/api/v1/projects/${projectId}/hosts/${hostId}/certificate`,
      { method: "PATCH", body: JSON.stringify(input) },
    ),

  renewHostCertificate: (projectId: string, hostId: string) =>
    request<{ certificate: DomainCertificate }>(
      `/api/v1/projects/${projectId}/hosts/${hostId}/certificate/renew`,
      { method: "POST" },
    ),

  importHostCertificate: (
    projectId: string,
    hostId: string,
    input: { certificate_pem: string; private_key_pem: string },
  ) =>
    request<{ certificate: DomainCertificate }>(
      `/api/v1/projects/${projectId}/hosts/${hostId}/certificate/import`,
      { method: "POST", body: JSON.stringify(input) },
    ),
};
