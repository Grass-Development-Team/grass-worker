import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { AdminRoute } from "./admin-route";
import { NodesPanel } from "./components/nodes-panel";

function renderAdmin(queryClient: QueryClient) {
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/admin/nodes"]}>
        <Routes>
          <Route path="/admin" element={<AdminRoute />}>
            <Route path="nodes" element={<NodesPanel />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

const localProcessFixture = {
  auto_start: true,
  managed: true,
  process: {
    state: "running",
    pid: 4242,
    started_at: new Date().toISOString(),
    restart_count: 0,
    last_exit_code: null,
    last_exit_at: null,
    message: null,
  },
};

it("loads system status through the protected administration API", async () => {
  const fetchMock = vi
    .spyOn(globalThis, "fetch")
    .mockImplementation(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/api/v1/admin/nodes")) {
        return jsonResponse({ nodes: [], local_process: localProcessFixture });
      }
      return jsonResponse({
        service: "Grass Worker Control API",
        mode: "ready",
        version: "9.9.9",
      });
    });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  renderAdmin(queryClient);

  expect(await screen.findByText("Ready mode · v9.9.9")).toBeInTheDocument();
  expect(fetchMock).toHaveBeenCalledWith(
    "/api/v1/admin/status",
    expect.objectContaining({ credentials: "include" }),
  );
});

it("lists nodes with health state for administrators", async () => {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("/api/v1/admin/nodes")) {
      return jsonResponse({
        nodes: [
          {
            id: "node-1",
            name: "build-node-1",
            status: "active",
            healthy: true,
            build_enabled: true,
            serve_enabled: true,
            build_concurrency: 2,
            base_url: "http://127.0.0.1:8080",
            work_root: "/data/node",
            version: "0.1.0",
            capacity: {
              cpu_millicores: 1600,
              memory_mb: 1536,
              disk_mb: 8192,
              max_deployments: 10,
            },
            usage: { cpu_millicores: 200, memory_mb: 256, disk_mb: 512, deployments: 1 },
            overflow_count: 0,
            last_heartbeat_at: new Date().toISOString(),
            created_at: new Date().toISOString(),
          },
        ],
        local_process: localProcessFixture,
      });
    }
    return jsonResponse({ service: "Grass Worker Control API", mode: "ready", version: "9.9.9" });
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  renderAdmin(queryClient);

  expect(await screen.findByText("build-node-1")).toBeInTheDocument();
  expect(screen.getByText("Healthy")).toBeInTheDocument();
  expect(screen.getByText("Local node process")).toBeInTheDocument();
});
