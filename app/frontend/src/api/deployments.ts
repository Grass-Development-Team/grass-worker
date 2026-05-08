import { request, requestText } from "./client";

export const deploymentsQueryKey = (projectId: string) =>
  ["projects", projectId, "deployments"] as const;
export const deploymentQueryKey = (projectId: string, deploymentId: string) =>
  ["projects", projectId, "deployments", deploymentId] as const;
export const deploymentArtifactsQueryKey = (projectId: string, deploymentId: string) =>
  ["projects", projectId, "deployments", deploymentId, "artifacts"] as const;
export const deploymentBuildLogQueryKey = (projectId: string, deploymentId: string) =>
  ["projects", projectId, "deployments", deploymentId, "build-log"] as const;

export type DeploymentStatus = "pending" | "processing" | "ready" | "failed" | "canceled";
export type DeploymentArtifactKind = "static_site" | "build_log";

export type Deployment = {
  id: string;
  project_id: string;
  status: DeploymentStatus;
  source_branch: string | null;
  source_revision: string | null;
  last_stage: string | null;
  failure_message: string | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
};

export type DeploymentArtifact = {
  id: string;
  deployment_id: string;
  kind: DeploymentArtifactKind;
  storage_path: string;
  checksum_sha256: string | null;
  size_bytes: number | null;
  created_at: string;
};

type DeploymentEnvelope = {
  deployment: Deployment;
};

type DeploymentsEnvelope = {
  deployments: Deployment[];
};

type DeploymentArtifactEnvelope = {
  artifact: DeploymentArtifact;
};

type DeploymentUploadEnvelope = {
  deployment: Deployment;
  artifact: DeploymentArtifact;
};

type DeploymentArtifactsEnvelope = {
  artifacts: DeploymentArtifact[];
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

export async function transitionProjectDeployment(
  projectId: string,
  deploymentId: string,
  input: { status: Exclude<DeploymentStatus, "pending"> },
): Promise<Deployment> {
  const response = await request<DeploymentEnvelope>(
    `/api/v1/projects/${projectId}/deployments/${deploymentId}/transition`,
    {
      method: "POST",
      body: JSON.stringify(input),
    },
  );
  return response.deployment;
}

export async function getProjectDeploymentArtifacts(
  projectId: string,
  deploymentId: string,
): Promise<DeploymentArtifact[]> {
  const response = await request<DeploymentArtifactsEnvelope>(
    `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts`,
  );
  return response.artifacts;
}

export async function getProjectDeploymentBuildLog(
  projectId: string,
  deploymentId: string,
): Promise<string> {
  return requestText(`/api/v1/projects/${projectId}/deployments/${deploymentId}/build-log`);
}

export async function createProjectDeploymentArtifact(
  projectId: string,
  deploymentId: string,
  input: {
    kind: DeploymentArtifactKind;
    storage_path: string;
    checksum_sha256?: string;
    size_bytes?: number;
  },
): Promise<DeploymentArtifact> {
  const response = await request<DeploymentArtifactEnvelope>(
    `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts`,
    {
      method: "POST",
      body: JSON.stringify(input),
    },
  );
  return response.artifact;
}

export async function uploadProjectDeploymentBundle(
  projectId: string,
  deploymentId: string,
  bundle: File,
): Promise<{ deployment: Deployment; artifact: DeploymentArtifact }> {
  const formData = new FormData();
  formData.append("bundle", bundle);

  const response = await request<DeploymentUploadEnvelope>(
    `/api/v1/projects/${projectId}/deployments/${deploymentId}/upload-bundle`,
    {
      method: "POST",
      body: formData,
    },
  );

  return {
    deployment: response.deployment,
    artifact: response.artifact,
  };
}
