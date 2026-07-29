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

const configuration = {
  node: {
    id: "serve-node-1",
    control_api: "http://127.0.0.1:7817",
    work_root: "/data/node",
    capabilities: { build: true, serve: true },
  },
  build: {
    concurrency: 2,
    command_timeout_seconds: 600,
    retain_workspace_on_failure: false,
  },
  serve: {
    host: "0.0.0.0",
    port: 8080,
    public_base_url: "http://127.0.0.1:8080",
    metadata_cache_ttl_seconds: 30,
    artifact_cache_root: "/data/node/artifacts",
    capacity: {
      cpu_millicores: 1200,
      memory_mb: 1536,
      disk_mb: 8192,
      max_deployments: 10,
    },
    ssr: { idle_stop_seconds: 1800, startup_timeout_seconds: 90 },
  },
  runtime: {
    backend: "podman-socket",
    socket: "unix:///run/user/1000/podman/podman.sock",
    default_build_image: "docker.io/library/node:22",
    default_serve_image: "docker.io/library/node:22",
    network: "bridge",
    resources: { cpu_limit: 2, memory_mb: 2048 },
  },
  security: { private_repository_targets: [] },
  development: { verbose_build_log: false },
  log: { level: "info", format: "pretty" },
};

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
  configuration: {
    desired: configuration,
    desired_revision: 3,
    effective: configuration,
    effective_revision: 3,
    status: "applied",
    error: null,
    node_token_configured: true,
    updated_at: "2026-07-27T00:00:00Z",
    applied_at: "2026-07-27T00:00:01Z",
  },
  last_heartbeat_at: "2026-07-27T00:00:00Z",
  created_at: "2026-07-27T00:00:00Z",
};

const targetNodeFixture = {
  ...nodeFixture,
  id: "node-2",
  name: "serve-node-2",
  base_url: "http://127.0.0.1:8081",
  usage: { cpu_millicores: 100, memory_mb: 128, disk_mb: 256, deployments: 0 },
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

it("edits the complete non-secret desired Node configuration", async () => {
  const calls: { url: string; init?: RequestInit }[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(
    async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, init });
      if (init?.method === "PUT") return jsonResponse({ node: nodeFixture });
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

  expect(await screen.findByText("Applied · r3")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Edit configuration for serve-node-1" }));
  const concurrencyInput = screen.getByLabelText("Build concurrency");
  await user.clear(concurrencyInput);
  await user.type(concurrencyInput, "4");
  await user.click(screen.getByRole("button", { name: "Save configuration" }));

  await waitFor(() => {
    const update = calls.find(
      (call) =>
        call.url === "/api/v1/admin/nodes/node-1/configuration" && call.init?.method === "PUT",
    );
    expect(update).toBeDefined();
    const payload = JSON.parse(String(update!.init!.body));
    expect(payload.build.concurrency).toBe(4);
    expect(payload.serve.capacity.max_deployments).toBe(10);
    expect(payload.runtime.default_serve_image).toBe("docker.io/library/node:22");
    expect(JSON.stringify(payload)).not.toContain("node_token");
  });
});

it("confirms deletion and selects a replacement before queueing a Serve Node", async () => {
  const calls: { url: string; init?: RequestInit }[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(
    async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, init });
      if (url.endsWith("/nodes/node-1/deletion-plan")) {
        return jsonResponse({
          node_id: "node-1",
          assigned_deployments: 1,
          active_builds: 0,
          requires_target: true,
          eligible_targets: [{ id: "node-2", name: "serve-node-2", available_deployments: 10 }],
        });
      }
      if (url.endsWith("/nodes/node-1/deletion") && init?.method === "POST") {
        return jsonResponse({
          job: {
            id: "job-1",
            status: "queued",
            target_node_id: "node-2",
            total_deployments: 1,
            migrated_deployments: 0,
            active_builds: 0,
            error: null,
          },
        });
      }
      return jsonResponse({
        nodes: [nodeFixture, targetNodeFixture],
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

  await user.click(await screen.findByRole("button", { name: "Delete serve-node-1" }));
  await user.click(screen.getByRole("button", { name: "Delete node" }));
  expect(await screen.findByText("Move services before deleting")).toBeInTheDocument();
  await user.click(screen.getByRole("combobox", { name: "Replacement Serve Node" }));
  await user.click(await screen.findByRole("option", { name: /serve-node-2/ }));
  await user.click(screen.getByRole("button", { name: "Queue deletion" }));

  await waitFor(() => {
    const request = calls.find(
      (call) => call.url.endsWith("/nodes/node-1/deletion") && call.init?.method === "POST",
    );
    expect(request).toBeDefined();
    expect(JSON.parse(String(request!.init!.body))).toEqual({ target_node_id: "node-2" });
  });
});

it("queues a Build Node directly after confirmation when no services are assigned", async () => {
  const calls: { url: string; init?: RequestInit }[] = [];
  const buildNode = {
    ...nodeFixture,
    name: "build-node-1",
    build_enabled: true,
    serve_enabled: false,
    build_concurrency: 2,
    base_url: null,
    usage: { cpu_millicores: 0, memory_mb: 0, disk_mb: 0, deployments: 0 },
  };
  vi.spyOn(globalThis, "fetch").mockImplementation(
    async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, init });
      if (url.endsWith("/nodes/node-1/deletion-plan")) {
        return jsonResponse({
          node_id: "node-1",
          assigned_deployments: 0,
          active_builds: 2,
          requires_target: false,
          eligible_targets: [],
        });
      }
      if (url.endsWith("/nodes/node-1/deletion") && init?.method === "POST") {
        return jsonResponse({
          job: {
            id: "job-1",
            status: "queued",
            target_node_id: null,
            total_deployments: 0,
            migrated_deployments: 0,
            active_builds: 2,
            error: null,
          },
        });
      }
      return jsonResponse({
        nodes: [buildNode],
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

  await user.click(await screen.findByRole("button", { name: "Delete build-node-1" }));
  await user.click(screen.getByRole("button", { name: "Delete node" }));

  await waitFor(() => {
    const request = calls.find(
      (call) => call.url.endsWith("/nodes/node-1/deletion") && call.init?.method === "POST",
    );
    expect(request).toBeDefined();
    expect(JSON.parse(String(request!.init!.body))).toEqual({ target_node_id: null });
  });
  expect(screen.queryByText("Move services before deleting")).not.toBeInTheDocument();
});

it("shows a failed deletion and allows an administrator to retry it", async () => {
  const calls: { url: string; init?: RequestInit }[] = [];
  const failedNode = {
    ...nodeFixture,
    deletion: {
      id: "job-1",
      status: "failed",
      target_node_id: "node-2",
      total_deployments: 1,
      migrated_deployments: 0,
      active_builds: 0,
      error: "artifact synchronization failed",
      created_at: "2026-07-27T00:00:00Z",
      updated_at: "2026-07-27T00:01:00Z",
      completed_at: null,
    },
  };
  vi.spyOn(globalThis, "fetch").mockImplementation(
    async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push({ url, init });
      if (url.endsWith("/nodes/node-1/deletion-plan")) {
        return jsonResponse({
          node_id: "node-1",
          assigned_deployments: 1,
          active_builds: 0,
          requires_target: true,
          eligible_targets: [{ id: "node-2", name: "serve-node-2", available_deployments: 10 }],
        });
      }
      if (url.endsWith("/nodes/node-1/deletion") && init?.method === "POST") {
        return jsonResponse({
          job: { ...failedNode.deletion, status: "queued", error: null },
        });
      }
      return jsonResponse({
        nodes: [failedNode, targetNodeFixture],
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

  expect(await screen.findByText("Deletion failed")).toBeInTheDocument();
  expect(screen.getByText("artifact synchronization failed")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Retry deletion for serve-node-1" }));
  await user.click(screen.getByRole("button", { name: "Delete node" }));
  await user.click(await screen.findByRole("combobox", { name: "Replacement Serve Node" }));
  await user.click(await screen.findByRole("option", { name: /serve-node-2/ }));
  await user.click(screen.getByRole("button", { name: "Queue deletion" }));

  await waitFor(() => {
    expect(
      calls.some(
        (call) => call.url.endsWith("/nodes/node-1/deletion") && call.init?.method === "POST",
      ),
    ).toBe(true);
  });
});
