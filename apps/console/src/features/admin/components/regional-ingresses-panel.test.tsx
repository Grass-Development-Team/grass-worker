import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { RegionalIngressesPanel } from "./regional-ingresses-panel";

const entry = {
  id: "entry-1",
  region: "hk_1",
  hostname: "hk.entry.example.com",
  enabled: true,
  health_check_path: "/_grass/health",
  health_check_interval_seconds: 30,
  dns_status: "unresolved",
  dns_error: "No public DNS address was found",
  dns_checked_at: null,
  healthy_nodes: [],
  node_statuses: [],
  created_at: "2026-09-11T00:00:00Z",
  updated_at: "2026-09-11T00:00:00Z",
};
function setup(items: unknown[] = [entry]) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = String(input);
    calls.push({ url, init });
    const data = url.endsWith("/regions")
      ? {
          regions: [
            {
              code: "hk_1",
              name: "hk_1",
              ingress_hostname: items.length ? entry.hostname : null,
              ingress_enabled: !!items.length,
            },
            { code: "hk_frick", name: "hk_frick", ingress_hostname: null, ingress_enabled: false },
          ],
        }
      : init?.method
        ? { regional_ingress: entry }
        : { regional_ingresses: items };
    return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  });
  render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
        })
      }
    >
      <RegionalIngressesPanel />
    </QueryClientProvider>,
  );
  return calls;
}
afterEach(() => vi.restoreAllMocks());

it("creates a manual CNAME entry from an existing region without DNS credentials or entry certificates", async () => {
  const user = userEvent.setup();
  const calls = setup([]);
  await user.click(screen.getByRole("button", { name: "Add regional entry" }));
  const dialog = screen.getByRole("dialog");
  expect(within(dialog).queryByRole("button", { name: "New Region" })).not.toBeInTheDocument();
  expect(
    within(dialog).queryByLabelText(/token|certificate authority|private key/i),
  ).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByLabelText("Region")).toBeEnabled());
  await user.click(screen.getByLabelText("Region"));
  await user.click(screen.getByRole("option", { name: "hk_frick" }));
  await user.type(screen.getByLabelText("CNAME target"), "hk.entry.example.com");
  await user.click(screen.getByRole("button", { name: "Create entry" }));
  await waitFor(() => expect(calls.some((c) => c.init?.method === "POST")).toBe(true));
  expect(JSON.parse(String(calls.find((c) => c.init?.method === "POST")!.init!.body))).toEqual({
    region: "hk_frick",
    hostname: "hk.entry.example.com",
    health_check_path: "/_grass/health",
    health_check_interval_seconds: 30,
  });
});
it("prevents reusing a region already assigned to an entry", async () => {
  const user = userEvent.setup();
  setup();
  await user.click(screen.getByRole("button", { name: "Add regional entry" }));
  await waitFor(() => expect(screen.getByLabelText("Region")).toBeEnabled());
  await user.click(screen.getByLabelText("Region"));
  expect(screen.getByRole("option", { name: /hk_1/ })).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByRole("option", { name: "hk_frick" })).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});
it("edits the target while keeping the assigned region fixed", async () => {
  const user = userEvent.setup();
  const calls = setup();
  await user.click(await screen.findByRole("button", { name: "Edit" }));
  expect(screen.getByLabelText("Region")).toBeDisabled();
  await user.clear(screen.getByLabelText("CNAME target"));
  await user.type(screen.getByLabelText("CNAME target"), "new.entry.example.com");
  await user.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(calls.some((c) => c.init?.method === "PATCH")).toBe(true));
  expect(JSON.parse(String(calls.find((c) => c.init?.method === "PATCH")!.init!.body))).toEqual({
    hostname: "new.entry.example.com",
    health_check_path: "/_grass/health",
    health_check_interval_seconds: 30,
  });
});
it("explains missing entry DNS and provides no entry certificate management", async () => {
  setup();
  expect(await screen.findByText("No public DNS address was found")).toBeInTheDocument();
  expect(screen.getByText(/Configure its DNS records manually/)).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: /renew|import certificate/i }),
  ).not.toBeInTheDocument();
});
