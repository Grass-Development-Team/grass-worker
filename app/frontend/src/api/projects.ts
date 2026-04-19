import { request } from "./client";

export const projectsQueryKey = ["projects"] as const;
export const projectQueryKey = (projectId: string) => ["projects", projectId] as const;

export type Project = {
  id: string;
  slug: string;
  name: string;
  status: "active" | "archived";
  created_at: string;
  updated_at: string;
  archived_at: string | null;
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
