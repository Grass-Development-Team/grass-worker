import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { DeploymentsTab } from "./deployments-tab";

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function renderDeployments() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <DeploymentsTab projectId="project-1" />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

it("creates with automatic placement by default and sends a selected serve node", async () => {
  const calls: { url: string; init?: RequestInit }[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(
    async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, init });
      if (url.endsWith("/serve-nodes")) {
        return jsonResponse({
          serve_nodes: [
            {
              id: "node-1",
              name: "serve-node-1",
              healthy: true,
              capacity: {
                cpu_millicores: 1000,
                memory_mb: 1024,
                disk_mb: 4096,
                max_deployments: 10,
              },
              usage: { cpu_millicores: 200, memory_mb: 256, disk_mb: 512, deployments: 1 },
              normal_available: true,
              schedulable: true,
              overflow_only: false,
              disk_available_mb: 3584,
              remaining_overflow_slots: 2,
            },
            {
              id: "node-2",
              name: "serve-node-2",
              healthy: true,
              capacity: {
                cpu_millicores: 2000,
                memory_mb: 2048,
                disk_mb: 8192,
                max_deployments: 10,
              },
              usage: { cpu_millicores: 400, memory_mb: 512, disk_mb: 1024, deployments: 2 },
              normal_available: true,
              schedulable: true,
              overflow_only: false,
              disk_available_mb: 7168,
              remaining_overflow_slots: 2,
            },
          ],
        });
      }
      if (init?.method === "POST") {
        return jsonResponse({ deployment: { id: "deployment-1" } });
      }
      return jsonResponse({ deployments: [] });
    },
  );
  const user = userEvent.setup();
  renderDeployments();

  await user.click(await screen.findByRole("button", { name: "Deploy preview" }));
  expect(await screen.findByRole("combobox", { name: "Serve node" })).toHaveTextContent(
    "Automatic",
  );
  await user.click(screen.getByRole("combobox", { name: "Serve node" }));
  await user.click(await screen.findByRole("option", { name: /serve-node-2/ }));
  await user.click(screen.getByRole("button", { name: "Create deployment" }));

  await waitFor(() => {
    const create = calls.find(
      (call) =>
        call.url === "/api/v1/projects/project-1/deployments" && call.init?.method === "POST",
    );
    expect(create).toBeDefined();
    expect(JSON.parse(String(create!.init!.body))).toMatchObject({
      environment: "preview",
      serve_node_id: "node-2",
    });
  });
});
