import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { HostSourcesPanel } from "./host-sources-panel";

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

const wildcardSource = {
  id: "source-1",
  kind: "wildcard",
  label: "Platform apps",
  base_domain: "apps.example.com",
  enabled: true,
  allows_auto_assign: true,
  is_default: true,
  provider: null,
  config_keys: [],
  created_at: "2026-07-01T00:00:00Z",
};

const cloudflareSource = {
  ...wildcardSource,
  id: "source-2",
  kind: "dns_provider",
  label: "Cloudflare zone",
  base_domain: "cf.example.com",
  is_default: false,
  provider: "cloudflare",
  config_keys: ["api_token", "zone_id", "record_type", "record_value", "proxied"],
};

function mockFetch() {
  const calls: { url: string; init?: RequestInit }[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(
    async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, init });
      if (init?.method === "POST" || init?.method === "PATCH") {
        return jsonResponse({ source: wildcardSource });
      }
      return jsonResponse({ sources: [wildcardSource, cloudflareSource] });
    },
  );
  return calls;
}

function renderPanel() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <HostSourcesPanel />
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

describe("Host sources panel", () => {
  it("creates a Cloudflare dns_provider source with its config", async () => {
    const calls = mockFetch();
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: /add host source/i }));
    await user.type(screen.getByLabelText("Label"), "CF zone");
    await user.click(screen.getByRole("combobox", { name: "Kind" }));
    await user.click(screen.getByRole("option", { name: /dns provider/i }));
    await user.type(screen.getByLabelText("Base domain"), "cf.example.com");
    await user.type(screen.getByLabelText("API token"), "cf-token");
    await user.type(screen.getByLabelText("Zone ID"), "zone123");
    await user.type(screen.getByLabelText("Record value"), "203.0.113.7");
    await user.click(screen.getByRole("button", { name: "Create source" }));

    const create = calls.find(
      (call) => call.url === "/api/v1/admin/host-sources" && call.init?.method === "POST",
    );
    expect(create).toBeDefined();
    const body = JSON.parse(String(create!.init!.body));
    expect(body.kind).toBe("dns_provider");
    expect(body.provider).toBe("cloudflare");
    expect(body.config).toMatchObject({
      api_token: "cf-token",
      zone_id: "zone123",
      record_type: "A",
      record_value: "203.0.113.7",
      proxied: false,
    });
  });

  it("edits a source without resending stored credentials", async () => {
    const calls = mockFetch();
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole("button", { name: "Edit Cloudflare zone" }));
    const label = screen.getByLabelText("Label");
    await user.clear(label);
    await user.type(label, "Cloudflare primary");
    await user.click(screen.getByRole("button", { name: "Save changes" }));

    const update = calls.find(
      (call) => call.url === "/api/v1/admin/host-sources/source-2" && call.init?.method === "PATCH",
    );
    expect(update).toBeDefined();
    const body = JSON.parse(String(update!.init!.body));
    expect(body.label).toBe("Cloudflare primary");
    expect(body.config).toBeUndefined();
  });
});
