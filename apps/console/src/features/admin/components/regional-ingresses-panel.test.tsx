import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { RegionalIngressesPanel } from "./regional-ingresses-panel";

const ingress = {
  id: "ingress-1",
  region: "eu-west",
  hostname: "eu.edge.example.com",
  enabled: true,
  health_check_path: "/_grass/health",
  health_check_interval_seconds: 30,
  origin_host_preservation: true,
  tls_enabled: true,
  certificate_issuer: "letsencrypt",
  certificate_auto_renew: true,
  certificate_status: "failed",
  certificate_expires_at: "2026-12-01T00:00:00Z",
  certificate_error: "DNS validation failed",
  dns_challenge_provider: "cloudflare",
  dns_challenge_config_keys: ["api_token", "zone_id"],
  dns_challenge_status: "failed",
  dns_challenge_record_name: null,
  dns_challenge_record_value: null,
  certificate_revision: "revision-1",
  healthy_nodes: [{ node_id: "node-1", base_url: "http://node-1:8080", priority: 0 }],
  node_statuses: [
    {
      node_id: "node-1",
      tls_ready: true,
      challenge_revision: "challenge-1",
      checked_at: "2026-09-10T00:00:00Z",
      certificate_revision: "revision-1",
    },
  ],
  created_at: "2026-09-10T00:00:00Z",
  updated_at: "2026-09-10T00:00:00Z",
};

function setup(items: unknown[] = [ingress]) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    calls.push({ url: String(input), init });
    return new Response(
      JSON.stringify({
        code: 200,
        message: "OK",
        data: init?.method ? { regional_ingress: ingress } : { regional_ingresses: items },
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
  });
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <RegionalIngressesPanel />
    </QueryClientProvider>,
  );
  return calls;
}

afterEach(() => vi.restoreAllMocks());

it("creates an ingress with usable DNS credentials and the reserved health path", async () => {
  const user = userEvent.setup();
  const calls = setup([]);
  await user.click(screen.getByRole("button", { name: "Add regional ingress" }));
  await user.type(screen.getByLabelText("Entry hostname"), "eu.edge.example.com");
  await user.type(screen.getByLabelText("Cloudflare API token"), "test-token");
  await user.type(screen.getByLabelText("Cloudflare zone ID"), "test-zone");
  await user.click(screen.getByRole("button", { name: "Create ingress" }));
  await waitFor(() => expect(calls.some((call) => call.init?.method === "POST")).toBe(true));
  const body = JSON.parse(String(calls.find((call) => call.init?.method === "POST")!.init!.body));
  expect(body).toMatchObject({
    hostname: "eu.edge.example.com",
    health_check_path: "/_grass/health",
    dns_challenge_provider: "cloudflare",
    dns_challenge_config: { api_token: "test-token", zone_id: "test-zone" },
  });
});

it("edits an ingress without replacing stored credentials with empty fields", async () => {
  const user = userEvent.setup();
  const calls = setup();
  await user.click(await screen.findByRole("button", { name: "Edit" }));
  expect(screen.getByLabelText("Cloudflare API token")).toHaveValue("");
  await user.clear(screen.getByLabelText("Entry hostname"));
  await user.type(screen.getByLabelText("Entry hostname"), "new.edge.example.com");
  await user.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(calls.some((call) => call.init?.method === "PATCH")).toBe(true));
  const body = JSON.parse(String(calls.find((call) => call.init?.method === "PATCH")!.init!.body));
  expect(body.hostname).toBe("new.edge.example.com");
  expect(body.dns_challenge_config).toEqual({});
});

it("shows installation and failure state and queues certificate renewal", async () => {
  const user = userEvent.setup();
  const calls = setup();
  expect(await screen.findByText("DNS validation failed")).toBeInTheDocument();
  expect(screen.getByText("1 using current certificate")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Renew / retry" }));
  await waitFor(() =>
    expect(
      calls.some(
        (call) => call.url.endsWith("/ingress-1/certificate/renew") && call.init?.method === "POST",
      ),
    ).toBe(true),
  );
});

it("removes an optional stored credential only when explicitly selected", async () => {
  const user = userEvent.setup();
  const calls = setup([
    { ...ingress, dns_challenge_config_keys: ["api_token", "zone_id", "contact_email"] },
  ]);
  await user.click(await screen.findByRole("button", { name: "Edit" }));
  await user.click(screen.getByRole("button", { name: "Remove Certificate contact email" }));
  await user.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(calls.some((call) => call.init?.method === "PATCH")).toBe(true));
  const body = JSON.parse(String(calls.find((call) => call.init?.method === "PATCH")!.init!.body));
  expect(body.dns_challenge_config).toEqual({ contact_email: null });
});

it("imports a manual certificate and clears key material after the dialog closes", async () => {
  const user = userEvent.setup();
  const calls = setup();
  await user.click(await screen.findByRole("button", { name: "Import certificate" }));
  const dialog = screen.getByRole("dialog");
  await user.type(within(dialog).getByLabelText("Certificate chain (PEM)"), "TEST CERTIFICATE");
  await user.type(within(dialog).getByLabelText("Private key (PEM)"), "TEST PRIVATE KEY");
  await user.click(within(dialog).getByRole("button", { name: "Import", exact: true }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  const request = calls.find((call) => call.url.endsWith("/certificate/import"))!;
  expect(JSON.parse(String(request.init!.body))).toEqual({
    certificate_pem: "TEST CERTIFICATE",
    private_key_pem: "TEST PRIVATE KEY",
  });
  await user.click(screen.getByRole("button", { name: "Import certificate" }));
  expect(screen.getByLabelText("Private key (PEM)")).toHaveValue("");
});
