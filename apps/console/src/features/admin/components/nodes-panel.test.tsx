import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { NodesPanel } from "./nodes-panel";

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

const nodeFixture = {
  id: "node-1",
  name: "serve-node-1",
  status: "active",
  healthy: true,
  build_enabled: false,
  serve_enabled: true,
  build_concurrency: 0,
  base_url: "http://127.0.0.1:8080",
  work_root: "/data/node",
  version: "0.1.0",
  capacity: {
    cpu_millicores: 1200,
    memory_mb: 1536,
    disk_mb: 8192,
    max_deployments: 10,
  },
  usage: { cpu_millicores: 200, memory_mb: 256, disk_mb: 512, deployments: 1 },
  overflow_count: 0,
  last_heartbeat_at: "2026-07-27T00:00:00Z",
  created_at: "2026-07-27T00:00:00Z",
};

function renderNodes() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <NodesPanel />
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

it("edits positive node scheduling capacity", async () => {
  const calls: { url: string; init?: RequestInit }[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(
    async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, init });
      if (init?.method === "PATCH") return jsonResponse({ node: nodeFixture });
      return jsonResponse({
        nodes: [nodeFixture],
        local_process: {
          auto_start: false,
          managed: false,
          process: {
            state: "stopped",
            pid: null,
            started_at: null,
            restart_count: 0,
            last_exit_code: null,
            last_exit_at: null,
            message: null,
          },
        },
      });
    },
  );
  const user = userEvent.setup();
  renderNodes();

  await user.click(await screen.findByRole("button", { name: "Edit capacity for serve-node-1" }));
  const cpuInput = screen.getByLabelText("CPU millicores");
  await user.clear(cpuInput);
  await user.type(cpuInput, "1600");
  await user.click(screen.getByRole("button", { name: "Save capacity" }));

  await waitFor(() => {
    const update = calls.find(
      (call) => call.url === "/api/v1/admin/nodes/node-1" && call.init?.method === "PATCH",
    );
    expect(update).toBeDefined();
    expect(JSON.parse(String(update!.init!.body))).toEqual({
      capacity_cpu_millicores: 1600,
      capacity_memory_mb: 1536,
      capacity_disk_mb: 8192,
      max_deployments: 10,
    });
  });
});
