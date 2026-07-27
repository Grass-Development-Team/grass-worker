import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { ProjectLayout } from "./project-layout";
import { ProjectOverviewRoute } from "./project-overview-route";

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

const projectFixture = {
  id: "p1",
  team_id: "team-1",
  slug: "landing",
  name: "Landing",
  runtime: "static",
  repository_url: "https://github.com/acme/landing.git",
  default_branch: "main",
  install_command: null,
  build_command: null,
  output_directory: null,
  source_config: {},
  build_config: {},
  archived_at: null,
  deleted_at: null,
  created_at: "2026-07-01T00:00:00Z",
  updated_at: "2026-07-01T00:00:00Z",
};

const activeDeployment = {
  id: "deploy-11112222",
  project_id: "p1",
  team_id: "team-1",
  environment: "production",
  runtime_kind: "static",
  build_status: "ready",
  serve_status: "ready",
  release_status: "active",
  build_node: { id: "node-1", name: "node-1" },
  serve_node: { id: "node-1", name: "node-1" },
  serve_resources: { cpu_millicores: 50, memory_mb: 64, disk_mb: 256 },
  overcommitted: false,
  build_stage: null,
  source: {
    repository_url: "https://github.com/acme/landing.git",
    branch: "main",
    commit_hash: "abcdef1234567890",
    commit_message: "Ship the landing page",
  },
  triggered_by: { id: "user-1", email: "dev@example.com", display_name: "Dev" },
  failure_code: null,
  failure_message: null,
  duration_seconds: 42,
  claimed_at: null,
  build_started_at: null,
  build_finished_at: null,
  created_at: "2026-07-02T00:00:00Z",
  preview_url: null,
  production_url: "http://landing.apps.example.com",
};

function mockFetch(deployments: unknown[]) {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("/deployments")) {
      return jsonResponse({ deployments });
    }
    if (url.includes("/hosts")) {
      return jsonResponse({
        hosts: [
          {
            id: "host-1",
            project_id: "p1",
            host: "landing.apps.example.com",
            kind: "platform",
            environment: "production",
            status: "active",
            failure_reason: null,
            is_primary: true,
            host_source_id: "source-1",
            created_at: "2026-07-01T00:00:00Z",
          },
        ],
      });
    }
    return jsonResponse({
      project: projectFixture,
      team: { id: "team-1", slug: "team", name: "Team" },
      role: "owner",
    });
  });
}

function renderOverview() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/projects/p1"]}>
        <Routes>
          <Route path="/projects/:projectId" element={<ProjectLayout />}>
            <Route index element={<ProjectOverviewRoute />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

describe("Project overview", () => {
  it("shows the active production deployment with domain and visit link", async () => {
    mockFetch([activeDeployment]);
    renderOverview();

    expect(await screen.findByText("Production Deployment")).toBeInTheDocument();
    expect(
      await screen.findByRole("link", { name: "landing.apps.example.com" }),
    ).toBeInTheDocument();
    const visit = await screen.findByRole("link", { name: /visit/i });
    expect(visit).toHaveAttribute("href", "http://landing.apps.example.com");
    expect(screen.getByText("Ship the landing page")).toBeInTheDocument();
  });

  it("shows an empty state when no production deployment exists", async () => {
    mockFetch([]);
    renderOverview();

    expect(await screen.findByText("No Production Deployment")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Go to Deployments" })).toHaveAttribute(
      "href",
      "/projects/p1/deployments",
    );
  });
});
