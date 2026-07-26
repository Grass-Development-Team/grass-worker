import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { AdminRoute } from "./admin-route";

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

  render(
    <QueryClientProvider client={queryClient}>
      <AdminRoute />
    </QueryClientProvider>,
  );

  expect(await screen.findByText("Ready mode | Version 9.9.9")).toBeInTheDocument();
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

  render(
    <QueryClientProvider client={queryClient}>
      <AdminRoute />
    </QueryClientProvider>,
  );

  expect(await screen.findByText("build-node-1")).toBeInTheDocument();
  expect(screen.getByText("Healthy")).toBeInTheDocument();
});
