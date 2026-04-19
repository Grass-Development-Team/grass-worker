import { request } from "./client";

export const projectsQueryKey = ["projects"] as const;

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

export async function getProjects(): Promise<Project[]> {
  const response = await request<ProjectsEnvelope>("/api/v1/projects");
  return response.projects;
}
