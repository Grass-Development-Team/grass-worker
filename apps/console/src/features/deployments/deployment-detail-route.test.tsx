import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

import { useTeam } from "@/features/teams/team-context";

import type { DeploymentDetail } from "./deployments.api";
import { DeploymentDetailRoute } from "./deployment-detail-route";

vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));

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
    was_active: false,
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
beforeEach(() => {
  vi.mocked(useTeam).mockReturnValue({ activeRole: "admin" } as ReturnType<typeof useTeam>);
});

it("keeps deployment actions hidden from viewers", async () => {
  vi.mocked(useTeam).mockReturnValue({ activeRole: "viewer" } as ReturnType<typeof useTeam>);
  renderDetail(
    detailFixture({
      build_status: "failed",
      serve_status: "failed",
      release_status: "draft",
    }),
  );

  expect(await screen.findByText("Failed")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Cancel build" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Promote" })).not.toBeInTheDocument();
});

it("keeps promote disabled while the serve node is syncing", async () => {
  renderDetail(detailFixture());

  expect(await screen.findByText("Syncing to serve node")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Promote" })).toBeDisabled();
  expect(screen.getByText("serve-node-1")).toBeInTheDocument();
  expect(screen.getByText("200m · 256 MB · 512 MB disk")).toBeInTheDocument();
});

it("does not expose platform moderation controls to team administrators", async () => {
  renderDetail(
    detailFixture({
      serve_status: "ready",
      release_status: "pending_review",
    }),
  );

  expect(await screen.findByText("Pending review")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Reject" })).not.toBeInTheDocument();
});

it("allows a team administrator to unpublish an active deployment", async () => {
  renderDetail(
    detailFixture({
      serve_status: "ready",
      release_status: "active",
      production_url: "https://landing.apps.example.com",
    }),
  );

  expect(await screen.findByRole("button", { name: "Unpublish" })).toBeEnabled();
});

it("does not let team members request platform moderation", async () => {
  renderDetail({
    ...detailFixture({
      serve_status: "ready",
      release_status: "draft",
    }),
    review_required: true,
  });

  expect(await screen.findByText("Draft")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Request review" })).not.toBeInTheDocument();
});

it("allows an administrator to queue rollback for a retired deployment", async () => {
  renderDetail({
    ...detailFixture({
      serve_node: null,
      serve_status: "retired",
      release_status: "approved",
      preview_url: null,
    }),
    was_active: true,
    events: [
      {
        id: "release-1",
        kind: "release",
        message: "previously active",
        metadata: {},
        created_at: "2026-07-27T00:00:20Z",
      },
    ],
  });

  expect(await screen.findByText("Retired")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Roll back to this deployment" })).toBeEnabled();
  expect(screen.queryByRole("link", { name: "Open deployment" })).not.toBeInTheDocument();
});

it("does not offer rollback for an approved deployment that was never active", async () => {
  renderDetail({
    ...detailFixture({
      serve_status: "ready",
      release_status: "approved",
    }),
    was_active: false,
    events: [
      {
        id: "release-approved",
        kind: "release",
        message: "release status changed to approved",
        metadata: {},
        created_at: "2026-07-27T00:00:20Z",
      },
    ],
  });

  expect(await screen.findByText("Approved")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Roll back to this deployment" }),
  ).not.toBeInTheDocument();
});
