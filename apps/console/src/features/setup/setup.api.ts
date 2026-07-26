import { request } from "@/lib/api";

export type SetupStage = "database" | "admin" | "site" | "node" | "storage" | "finish" | "complete";

export interface SetupState {
  stage: SetupStage;
  is_setup_mode: boolean;
}

export function buildPostgresUrl(
  host: string,
  port: string,
  username: string,
  password: string,
  database: string,
): string {
  const normalizedHost = host.trim();
  const url = new URL("postgres://placeholder");
  url.hostname = normalizedHost.includes(":")
    ? normalizedHost.startsWith("[")
      ? normalizedHost
      : `[${normalizedHost}]`
    : normalizedHost;
  if (url.hostname === "placeholder" && normalizedHost !== "placeholder") {
    throw new Error("Database host is invalid.");
  }
  url.port = port.trim();
  url.username = username.trim();
  url.password = password;
  url.pathname = `/${encodeURIComponent(database.trim())}`;
  return url.toString();
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
    const url = buildPostgresUrl(host, port, username, password, database);
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

  configureSite: (name: string, site_url: string, public_base_url: string) =>
    request<{ configured: boolean; name: string; site_url: string; public_base_url: string }>(
      "/api/v1/setup/site",
      {
        method: "POST",
        body: JSON.stringify({ name, site_url, public_base_url }),
      },
    ),

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
