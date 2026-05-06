import { request } from "./client";

export const adminPlatformHostSourcesQueryKey = ["admin", "platform-host-sources"] as const;

export type PlatformHostSourceKind = "wildcard_static" | "dns_managed";

export type PlatformHostSource = {
  id: string;
  kind: PlatformHostSourceKind;
  label: string;
  base_domain: string;
  enabled: boolean;
  allows_auto_assign: boolean;
  created_at: string;
  updated_at: string;
};

export type CreatePlatformHostSourceInput = {
  kind: PlatformHostSourceKind;
  label: string;
  base_domain: string;
  enabled: boolean;
  allows_auto_assign: boolean;
};

export const projectHostBindingsQueryKey = (projectId: string) =>
  ["projects", projectId, "hosts"] as const;

export type ProjectHostBinding = {
  id: string;
  project_id: string;
  source_id: string | null;
  host: string;
  is_primary: boolean;
  created_at: string;
  updated_at: string;
};

export type CreateProjectHostBindingInput = {
  host: string;
  source_id?: string;
  is_primary?: boolean;
};

type PlatformHostSourceEnvelope = {
  source: PlatformHostSource;
};

type PlatformHostSourcesEnvelope = {
  sources: PlatformHostSource[];
};

type ProjectHostBindingEnvelope = {
  host: ProjectHostBinding;
};

type ProjectHostBindingsEnvelope = {
  hosts: ProjectHostBinding[];
};

export async function getAdminPlatformHostSources(): Promise<PlatformHostSource[]> {
  const response = await request<PlatformHostSourcesEnvelope>(
    "/api/v1/admin/platform-host-sources",
  );
  return response.sources;
}

export async function createPlatformHostSource(
  input: CreatePlatformHostSourceInput,
): Promise<PlatformHostSource> {
  const response = await request<PlatformHostSourceEnvelope>(
    "/api/v1/admin/platform-host-sources",
    {
      method: "POST",
      body: JSON.stringify(input),
    },
  );
  return response.source;
}

export async function enablePlatformHostSource(sourceId: string): Promise<PlatformHostSource> {
  const response = await request<PlatformHostSourceEnvelope>(
    `/api/v1/admin/platform-host-sources/${sourceId}/enable`,
    {
      method: "POST",
    },
  );
  return response.source;
}

export async function disablePlatformHostSource(sourceId: string): Promise<PlatformHostSource> {
  const response = await request<PlatformHostSourceEnvelope>(
    `/api/v1/admin/platform-host-sources/${sourceId}/disable`,
    {
      method: "POST",
    },
  );
  return response.source;
}

export async function getProjectHostBindings(projectId: string): Promise<ProjectHostBinding[]> {
  const response = await request<ProjectHostBindingsEnvelope>(
    `/api/v1/projects/${projectId}/hosts`,
  );
  return response.hosts;
}

export async function createProjectHostBinding(
  projectId: string,
  input: CreateProjectHostBindingInput,
): Promise<ProjectHostBinding> {
  const response = await request<ProjectHostBindingEnvelope>(
    `/api/v1/projects/${projectId}/hosts`,
    {
      method: "POST",
      body: JSON.stringify(input),
    },
  );
  return response.host;
}

export async function setProjectPrimaryHost(
  projectId: string,
  bindingId: string,
): Promise<ProjectHostBinding> {
  const response = await request<ProjectHostBindingEnvelope>(
    `/api/v1/projects/${projectId}/hosts/${bindingId}/primary`,
    {
      method: "POST",
    },
  );
  return response.host;
}

export async function deleteProjectHostBinding(
  projectId: string,
  bindingId: string,
): Promise<void> {
  await request<void>(`/api/v1/projects/${projectId}/hosts/${bindingId}`, {
    method: "DELETE",
  });
}
