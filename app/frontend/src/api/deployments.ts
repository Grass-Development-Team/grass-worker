import { request } from "./client";

export const deploymentsQueryKey = (projectId: string) =>
  ["projects", projectId, "deployments"] as const;
export const deploymentQueryKey = (projectId: string, deploymentId: string) =>
  ["projects", projectId, "deployments", deploymentId] as const;

export type DeploymentStatus = "pending" | "processing" | "ready" | "failed" | "canceled";

export type Deployment = {
  id: string;
  project_id: string;
  status: DeploymentStatus;
  source_branch: string | null;
  source_revision: string | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
};

type DeploymentEnvelope = {
  deployment: Deployment;
};

type DeploymentsEnvelope = {
  deployments: Deployment[];
};

export async function getProjectDeployments(projectId: string): Promise<Deployment[]> {
  const response = await request<DeploymentsEnvelope>(
    `/api/v1/projects/${projectId}/deployments`,
  );
  return response.deployments;
}

export async function createProjectDeployment(
  projectId: string,
  input: { source_branch?: string; source_revision?: string },
): Promise<Deployment> {
  const response = await request<DeploymentEnvelope>(
    `/api/v1/projects/${projectId}/deployments`,
    {
      method: "POST",
      body: JSON.stringify(input),
    },
  );
  return response.deployment;
}

export async function getProjectDeployment(
  projectId: string,
  deploymentId: string,
): Promise<Deployment> {
  const response = await request<DeploymentEnvelope>(
    `/api/v1/projects/${projectId}/deployments/${deploymentId}`,
  );
  return response.deployment;
}
