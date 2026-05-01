import type { Deployment } from "./deployments";
import { request } from "./client";

export const projectReleaseQueryKey = (projectId: string) =>
  ["projects", projectId, "release"] as const;

export type Release = {
  project_id: string;
  project_slug: string;
  site_url: string;
  active_deployment_id: string | null;
  active_deployment: Deployment | null;
  rollback_deployment_id: string | null;
};

type ReleaseEnvelope = {
  release: Release;
};

export async function getProjectRelease(projectId: string): Promise<Release> {
  const response = await request<ReleaseEnvelope>(`/api/v1/projects/${projectId}/release`);
  return response.release;
}

export async function activateProjectRelease(
  projectId: string,
  deploymentId: string,
): Promise<Release> {
  const response = await request<ReleaseEnvelope>(
    `/api/v1/projects/${projectId}/release/activate`,
    {
      method: "POST",
      body: JSON.stringify({ deployment_id: deploymentId }),
    },
  );
  return response.release;
}

export async function rollbackProjectRelease(projectId: string): Promise<Release> {
  const response = await request<ReleaseEnvelope>(
    `/api/v1/projects/${projectId}/release/rollback`,
    {
      method: "POST",
    },
  );
  return response.release;
}
