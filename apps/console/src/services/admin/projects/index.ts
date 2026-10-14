import { request } from "@/infra/api/index";
import type { AuditPage } from "@/services/audit/index";
import type { ReleaseStatus } from "@/services/deployments/index";
import type {
  AdminBatchItemResult,
  AdminDeploymentGovernanceResult,
  AdminDomainGovernanceResult,
  AdminGovernanceDeployment,
  AdminProject,
  AdminProjectDetail,
  AdminProjectDomain,
  AdminProjectFilters,
  AdminProjectTeamRef,
} from "./model";
export const projectsApi = {
  listProjects: (filters: AdminProjectFilters = {}) => {
    const query = new URLSearchParams();
    if (filters.q?.trim()) query.set("q", filters.q.trim());
    if (filters.status) query.set("status", filters.status);
    const suffix = query.size ? `?${query}` : "";
    return request<{ projects: AdminProject[] }>(`/api/v1/admin/projects${suffix}`);
  },

  batchProjects: (input: { action: "archive" | "unarchive" | "delete"; ids: string[] }) =>
    request<{ results: AdminBatchItemResult[] }>("/api/v1/admin/projects/batch", {
      method: "POST",
      body: JSON.stringify(input),
    }),

  getProject: (projectId: string) =>
    request<{ project: AdminProjectDetail; team: AdminProjectTeamRef | null }>(
      `/api/v1/admin/projects/${projectId}`,
    ),

  updateProjectSlug: (projectId: string, slug: string, reason?: string) =>
    request<{
      project: Pick<AdminProjectDetail, "id" | "uuid" | "slug">;
      reason: string | null;
    }>(`/api/v1/admin/projects/${projectId}/slug`, {
      method: "PATCH",
      body: JSON.stringify({ slug, ...(reason ? { reason } : {}) }),
    }),

  listProjectDeployments: (projectId: string) =>
    request<{ deployments: AdminGovernanceDeployment[] }>(
      `/api/v1/admin/projects/${projectId}/deployments`,
    ),

  withdrawDeployment: (deploymentId: string, reason?: string) =>
    request<AdminDeploymentGovernanceResult>(`/api/v1/admin/deployments/${deploymentId}/withdraw`, {
      method: "POST",
      body: JSON.stringify(reason ? { reason } : {}),
    }),

  republishDeployment: (deploymentId: string, reason?: string) =>
    request<
      AdminDeploymentGovernanceResult & {
        release_status: ReleaseStatus;
        release_pending: boolean;
        review_id: string | null;
      }
    >(`/api/v1/admin/deployments/${deploymentId}/republish`, {
      method: "POST",
      body: JSON.stringify(reason ? { reason } : {}),
    }),

  listProjectDomains: (projectId: string) =>
    request<{ domains: AdminProjectDomain[] }>(`/api/v1/admin/projects/${projectId}/domains`),

  approveDomain: (domainId: string, reason?: string) =>
    request<AdminDomainGovernanceResult>(`/api/v1/admin/domains/${domainId}/approve`, {
      method: "POST",
      body: JSON.stringify(reason ? { reason } : {}),
    }),

  rejectDomain: (domainId: string, reason?: string) =>
    request<AdminDomainGovernanceResult>(`/api/v1/admin/domains/${domainId}/reject`, {
      method: "POST",
      body: JSON.stringify(reason ? { reason } : {}),
    }),

  deleteDomain: (domainId: string, reason?: string) =>
    request<{ deleted: true; reason: string | null }>(`/api/v1/admin/domains/${domainId}`, {
      method: "DELETE",
      body: JSON.stringify(reason ? { reason } : {}),
    }),

  listProjectActivity: (projectId: string, page = 1) =>
    request<AuditPage>(`/api/v1/admin/projects/${projectId}/activity?page=${page}`),

  archiveProject: (projectId: string) =>
    request<{ project: AdminProject }>(`/api/v1/admin/projects/${projectId}/archive`, {
      method: "POST",
    }),

  unarchiveProject: (projectId: string) =>
    request<{ project: AdminProject }>(`/api/v1/admin/projects/${projectId}/unarchive`, {
      method: "POST",
    }),

  deleteProject: (projectId: string) =>
    request<{ deleted: true }>(`/api/v1/admin/projects/${projectId}/delete`, { method: "POST" }),

  restoreProject: (projectId: string) =>
    request<{ project: AdminProject }>(`/api/v1/admin/projects/${projectId}/restore`, {
      method: "POST",
    }),
};
