import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { deploymentRefetchInterval, type Deployment } from "./deployments.api";
import { DeploymentsTab } from "./deployments-tab";

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function renderDeployments(canDeploy = true) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <DeploymentsTab projectId="project-1" canDeploy={canDeploy} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

it("keeps deployments read-only when creation is not allowed", async () => {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse({ deployments: [] }));

  renderDeployments(false);

  await screen.findByText("No deployments yet.");
  expect(screen.queryByRole("button", { name: "Deploy preview" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Deploy production" })).not.toBeInTheDocument();
});

afterEach(() => vi.restoreAllMocks());

function deploymentFixture(overrides: Partial<Deployment> = {}): Deployment {
  return {
    id: "deployment-1",
    project_id: "project-1",
    team_id: "team-1",
    build_node: { id: "build-node-1", name: "builder-1" },
    serve_node: { id: "serve-node-1", name: "serve-node-1" },
    environment: "production",
    runtime_kind: "static",
    build_status: "ready",
    serve_status: "failed",
    release_status: "draft",
    serve_resources: { cpu_millicores: 50, memory_mb: 64, disk_mb: 256 },
    overcommitted: false,
    build_stage: null,
    source: {
      repository_url: "https://example.com/repository.git",
      branch: "main",
      commit_hash: "1234567890abcdef",
      commit_message: "Deploy application",
    },
    triggered_by: null,
    failure_code: null,
    failure_message: null,
    serve_failure_code: "artifact_checksum_mismatch",
    serve_failure_message: "Artifact checksum mismatch",
    duration_seconds: 12,
    claimed_at: null,
    build_started_at: null,
    build_finished_at: "2026-07-27T00:00:10Z",
    serve_started_at: "2026-07-27T00:00:11Z",
    serve_finished_at: "2026-07-27T00:00:12Z",
    created_at: "2026-07-27T00:00:00Z",
    preview_url: null,
    production_url: null,
    ...overrides,
  };
}

it("keeps polling while build or serve work is in progress", () => {
  expect(deploymentRefetchInterval({ build_status: "building", serve_status: "pending" })).toBe(
    4000,
  );
  expect(deploymentRefetchInterval({ build_status: "ready", serve_status: "pending" })).toBe(4000);
  expect(deploymentRefetchInterval({ build_status: "ready", serve_status: "syncing" })).toBe(4000);
  expect(deploymentRefetchInterval({ build_status: "ready", serve_status: "ready" })).toBe(false);
  expect(deploymentRefetchInterval({ build_status: "failed", serve_status: "pending" })).toBe(
    false,
  );
});

it("shows serve placement and serve failures in the serve column", async () => {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(
    jsonResponse({ deployments: [deploymentFixture()] }),
  );

  renderDeployments();

  expect(await screen.findByText("Serve failed")).toBeInTheDocument();
  expect(screen.getByText("serve-node-1")).toBeInTheDocument();
  expect(screen.getByText("50m · 64MB · 256 MB disk")).toBeInTheDocument();
  expect(screen.getByText("Artifact checksum mismatch")).toBeInTheDocument();
});

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
