import { request } from "./client";

export const projectsQueryKey = ["projects"] as const;
export const projectQueryKey = (projectId: string) => ["projects", projectId] as const;

export type ProjectStatus = "active" | "archived" | "soft_deleted";

export type Project = {
  id: string;
  owner_user_id: string;
  slug: string;
  name: string;
  status: ProjectStatus;
  created_at: string;
  updated_at: string;
  archived_at: string | null;
  soft_deleted_at: string | null;
};

type ProjectsEnvelope = {
  projects: Project[];
};

type ProjectRecordEnvelope = {
  project: Project;
};

export async function getProjects(): Promise<Project[]> {
  const response = await request<ProjectsEnvelope>("/api/v1/projects");
  return response.projects;
}

export async function createProject(input: {
  name: string;
  slug: string;
}): Promise<Project> {
  const response = await request<ProjectRecordEnvelope>("/api/v1/projects", {
    method: "POST",
    body: JSON.stringify(input),
  });

  return response.project;
}

export async function getProject(projectId: string): Promise<Project> {
  const response = await request<ProjectRecordEnvelope>(`/api/v1/projects/${projectId}`);
  return response.project;
}

export async function updateProject(
  projectId: string,
  input: { name?: string; slug?: string },
): Promise<Project> {
  const response = await request<ProjectRecordEnvelope>(`/api/v1/projects/${projectId}`, {
    method: "PATCH",
    body: JSON.stringify(input),
  });

  return response.project;
}

export async function archiveProject(projectId: string): Promise<Project> {
  const response = await request<ProjectRecordEnvelope>(
    `/api/v1/projects/${projectId}/archive`,
    { method: "POST" },
  );

  return response.project;
}

export async function unarchiveProject(projectId: string): Promise<Project> {
  const response = await request<ProjectRecordEnvelope>(
    `/api/v1/projects/${projectId}/unarchive`,
    { method: "POST" },
  );

  return response.project;
}

export async function softDeleteProject(projectId: string): Promise<Project> {
  const response = await request<ProjectRecordEnvelope>(
    `/api/v1/projects/${projectId}/soft-delete`,
    { method: "POST" },
  );

  return response.project;
}

export async function restoreProject(
  projectId: string,
  status: "active" | "archived",
): Promise<Project> {
  const response = await request<ProjectRecordEnvelope>(
    `/api/v1/projects/${projectId}/restore`,
    {
      method: "POST",
      body: JSON.stringify({ status }),
    },
  );

  return response.project;
}

export async function transferProjectOwner(
  projectId: string,
  ownerEmail: string,
): Promise<Project> {
  const response = await request<ProjectRecordEnvelope>(
    `/api/v1/projects/${projectId}/transfer-owner`,
    {
      method: "POST",
      body: JSON.stringify({ owner_email: ownerEmail }),
    },
  );

  return response.project;
}

export async function hardDeleteProject(projectId: string): Promise<void> {
  await request<void>(`/api/v1/projects/${projectId}/hard-delete`, {
    method: "POST",
  });
}
