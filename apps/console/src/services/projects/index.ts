import { request } from "@/infra/api/index";
import type {
  BoundSourceCredential,
  CreateProjectInput,
  DomainCertificate,
  HostAssignment,
  HostEnvironment,
  Project,
  ProjectHost,
  UpdateProjectInput,
} from "./model";
export type * from "./model";

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
