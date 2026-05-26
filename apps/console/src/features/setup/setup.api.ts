import { request } from "@/lib/api";

export type SetupStage = "database" | "admin" | "site" | "node" | "storage" | "finish" | "complete";

export interface SetupState {
  stage: SetupStage;
  is_setup_mode: boolean;
}

export const setupApi = {
  getSetupState: () => request<SetupState>("/api/v1/setup/state"),

  configureDatabase: (
    host: string,
    port: string,
    username: string,
    password: string,
    database: string,
  ) => {
    const url = `postgres://${encodeURIComponent(username)}:${encodeURIComponent(password)}@${host}:${port}/${database}`;
    return request<{ connected: boolean; migrations_applied: boolean; seed_completed: boolean }>(
      "/api/v1/setup/database",
      { method: "POST", body: JSON.stringify({ url }) },
    );
  },

  createAdmin: (email: string, password: string, display_name?: string) =>
    request<{
      user: { id: string; email: string; display_name: string };
      team: { id: string; slug: string; name: string };
    }>("/api/v1/setup/admin", {
      method: "POST",
      body: JSON.stringify({ email, password, display_name }),
    }),

  configureSite: (name: string) =>
    request<{ configured: boolean; name: string }>("/api/v1/setup/site", {
      method: "POST",
      body: JSON.stringify({ name }),
    }),

  createNode: (name?: string) =>
    request<{ node: { id: string; name: string }; token: string }>("/api/v1/setup/node", {
      method: "POST",
      body: JSON.stringify({ name: name ?? null }),
    }),

  configureStorage: (root?: string) =>
    request<{ configured: boolean; root: string }>("/api/v1/setup/storage", {
      method: "POST",
      body: JSON.stringify({ root: root ?? null }),
    }),

  finishSetup: () =>
    request<{ setup_finished: boolean }>("/api/v1/setup/finish", { method: "POST" }),
};
