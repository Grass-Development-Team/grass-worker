import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import type { DeploymentDetail } from "./deployments.api";
import { DeploymentDetailRoute } from "./deployment-detail-route";

vi.mock("@/features/teams/team-context", () => ({
  useTeam: () => ({ activeRole: "admin" }),
}));

vi.mock("./components/log-viewer", () => ({
  LogViewer: () => null,
}));

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function detailFixture(overrides: Partial<DeploymentDetail["deployment"]> = {}): DeploymentDetail {
  return {
    deployment: {
      id: "deployment-1",
      project_id: "project-1",
      team_id: "team-1",
      build_node: { id: "build-node-1", name: "builder-1" },
      serve_node: { id: "serve-node-1", name: "serve-node-1" },
      environment: "production",
      runtime_kind: "ssr",
      build_status: "ready",
      serve_status: "syncing",
      release_status: "draft",
      serve_resources: { cpu_millicores: 200, memory_mb: 256, disk_mb: 512 },
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
      serve_failure_code: null,
      serve_failure_message: null,
      duration_seconds: 12,
      claimed_at: null,
      build_started_at: null,
      build_finished_at: "2026-07-27T00:00:10Z",
      serve_started_at: "2026-07-27T00:00:11Z",
      serve_finished_at: null,
      created_at: "2026-07-27T00:00:00Z",
      preview_url: null,
      production_url: null,
      ...overrides,
    },
    events: [],
    artifacts: [],
    reviews: [],
    review_required: false,
  };
}

function renderDetail(detail: DeploymentDetail) {
  vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse(detail));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/projects/project-1/deployments/deployment-1"]}>
        <Routes>
          <Route
            path="/projects/:projectId/deployments/:deploymentId"
            element={<DeploymentDetailRoute />}
          />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

it("keeps promote disabled while the serve node is syncing", async () => {
  renderDetail(detailFixture());

  expect(await screen.findByText("Syncing to serve node")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Promote" })).toBeDisabled();
  expect(screen.getByText("serve-node-1")).toBeInTheDocument();
  expect(screen.getByText("200m · 256 MB · 512 MB disk")).toBeInTheDocument();
});
