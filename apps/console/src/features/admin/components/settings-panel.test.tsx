import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { adminApi, type AdminSettings } from "../admin.api";
import { SettingsPanel } from "./settings-panel";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      getSettings: vi.fn(),
      updateSettings: vi.fn(),
    },
  };
});

const settings = {
  site: {
    name: "Old Name",
    url: "https://console.example.com",
    public_base_url: "https://apps.example.com",
  },
  storage: { root: "/var/lib/grass-worker" },
  signup: { policy: "open" },
  review: { production: "manual", preview: "auto" },
  domain_review: { default: "auto" },
  server: { host: "127.0.0.1", port: 7817 },
  database: { url_configured: true },
  redis: { backend: "redis", url_configured: true },
  secrets: { secret_key_configured: true, git_credentials_configured: false },
  session: { cookie_secure: true, idle_ttl_seconds: 900, session_ttl_seconds: 2_592_000 },
  audit: { retention_days: 90 },
  node_manager: {
    auto_start_local_node: false,
    local_node_binary: "grass-node",
    local_node_config: "./node.toml",
    restart_on_exit: true,
  },
  migration: { auto_migrate: false },
  log: { level: "info", format: "pretty" },
  restart_required_sections: ["server", "redis", "node_manager", "migration", "log"],
} as AdminSettings;

beforeEach(() => {
  vi.mocked(adminApi.getSettings).mockResolvedValue(settings);
  vi.mocked(adminApi.updateSettings).mockResolvedValue({
    ...settings,
    site: { ...settings.site, name: "Acme Deploy" },
  });
});

it("invalidates the public site configuration after saving the site name", async () => {
  const user = userEvent.setup();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData(["site-config"], { site_name: "Old Name", version: "0.1.0" });
  render(
    <QueryClientProvider client={client}>
      <SettingsPanel />
    </QueryClientProvider>,
  );

  const name = await screen.findByLabelText("Site name");
  await user.clear(name);
  await user.type(name, "Acme Deploy");
  await user.click(within(name.closest("form")!).getByRole("button", { name: "Save" }));

  await waitFor(() => expect(adminApi.updateSettings).toHaveBeenCalled());
  expect(client.getQueryState(["site-config"])?.isInvalidated).toBe(true);
});

it("shows every non-secret Control API setting and only secret configuration status", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <SettingsPanel />
    </QueryClientProvider>,
  );

  expect(await screen.findByLabelText("Server host")).toHaveValue("127.0.0.1");
  expect(screen.getByLabelText("Server port")).toHaveValue(7817);
  expect(screen.getByLabelText("Cache backend")).toHaveTextContent("Redis");
  expect(screen.getByLabelText("Secure session cookies")).toBeChecked();
  expect(screen.getByLabelText("Session idle TTL (seconds)")).toHaveValue(900);
  expect(screen.getByLabelText("Absolute session TTL (seconds)")).toHaveValue(2_592_000);
  expect(screen.getByLabelText("Audit retention (days)")).toHaveValue(90);
  expect(screen.getByLabelText("Auto-start local Node")).not.toBeChecked();
  expect(screen.getByLabelText("Local Node binary")).toHaveValue("grass-node");
  expect(screen.getByLabelText("Local Node config")).toHaveValue("./node.toml");
  expect(screen.getByLabelText("Restart local Node on exit")).toBeChecked();
  expect(screen.getByLabelText("Run database migrations on startup")).not.toBeChecked();
  expect(screen.getByLabelText("Log filter")).toHaveValue("info");
  expect(screen.getByLabelText("Log format")).toHaveTextContent("Pretty");

  const sensitive = screen.getByText("Sensitive configuration").closest("[data-slot='card']");
  expect(sensitive).not.toBeNull();
  expect(within(sensitive!).getByText("Database URL")).toBeInTheDocument();
  expect(within(sensitive!).getByText("Redis URL")).toBeInTheDocument();
  expect(within(sensitive!).getByText("Control API secret")).toBeInTheDocument();
  expect(within(sensitive!).getByText("Git credential encryption")).toBeInTheDocument();
  expect(within(sensitive!).getAllByText("Configured")).toHaveLength(3);
  expect(within(sensitive!).getByText("Not configured")).toBeInTheDocument();
});

it("shows and saves the custom domain review default", async () => {
  const user = userEvent.setup();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <SettingsPanel />
    </QueryClientProvider>,
  );

  const domainReview = await screen.findByRole("combobox", { name: "Custom domain review" });
  expect(domainReview).toHaveTextContent("Auto");
  await user.click(domainReview);
  await user.click(screen.getByRole("option", { name: /Manual/ }));
  await user.click(within(domainReview.closest("form")!).getByRole("button", { name: "Save" }));

  await waitFor(() =>
    expect(adminApi.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ domain_review_default: "manual" }),
    ),
  );
});
