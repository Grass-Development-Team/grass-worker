import { request } from "./client";

export const systemInfoQueryKey = ["system-info"] as const;

export type SetupStage = "database" | "admin";

export type ReadySystemInfo = {
  service: string;
  mode: "ready";
};

export type SetupSystemInfo = {
  service: string;
  mode: "setup";
  stage: SetupStage;
  status: string;
};

export type SystemInfo = ReadySystemInfo | SetupSystemInfo;

type SetupActionResponse = {
  stage: SetupStage;
  status: string;
};

export type DatabaseSetupInput = {
  host: string;
  port: number;
  db_name: string;
  user: string;
  password: string;
  schema: string;
};

export type AdminSetupInput = {
  email: string;
  password: string;
};

export async function getSystemInfo(): Promise<SystemInfo> {
  return request<SystemInfo>("/api/v1/info");
}

export async function submitDatabaseSetup(
  input: DatabaseSetupInput,
): Promise<SetupActionResponse> {
  return request<SetupActionResponse>("/api/v1/setup/database", {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export async function submitAdminSetup(
  input: AdminSetupInput,
): Promise<SetupActionResponse> {
  return request<SetupActionResponse>("/api/v1/setup/admin", {
    method: "POST",
    body: JSON.stringify(input),
  });
}
