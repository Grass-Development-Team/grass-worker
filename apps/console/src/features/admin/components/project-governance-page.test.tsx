import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, expect, it, vi } from "vitest";

import { adminApi } from "../admin.api";
import { ProjectGovernancePage } from "./project-governance-page";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      getProject: vi.fn(),
      updateProjectSlug: vi.fn(),
      listProjectDeployments: vi.fn(),
      withdrawDeployment: vi.fn(),
      republishDeployment: vi.fn(),
      listProjectDomains: vi.fn(),
      approveDomain: vi.fn(),
      rejectDomain: vi.fn(),
      deleteDomain: vi.fn(),
      listProjectActivity: vi.fn(),
    },
  };
});

const project = {
  id: "project-1",
  uuid: "project-1",
  slug: "demo-site",
  name: "Demo Site",
  runtime: "static",
  repository_url: "https://example.com/acme/demo.git",
  default_branch: "main",
  install_command: "vp install",
  build_command: "vp build",
  output_directory: "dist",
  source_config: {},
  build_config: {},
  archived_at: null,
  created_at: "2026-07-30T00:00:00Z",
  updated_at: "2026-07-30T00:00:00Z",
};

const deployment = {
  id: "deployment-1",
  project_id: "project-1",
  environment: "production" as const,
  build_status: "ready" as const,
  serve_status: "ready" as const,
  release_status: "active" as const,
  release_pending: false,
  preview_host: "deployment-1.preview.example.com",
  source_repository_url: project.repository_url,
  source_branch: "main",
  commit_hash: "1234567890abcdef",
  commit_message: "Ship governance",
  build_stage: null,
  failure_code: null,
  failure_message: null,
  serve_failure_code: null,
  serve_failure_message: null,
  claimed_at: null,
  build_started_at: null,
  build_finished_at: null,
  serve_started_at: null,
  serve_finished_at: null,
  created_at: "2026-07-30T01:00:00Z",
  updated_at: "2026-07-30T01:00:00Z",
};

const domain = {
  id: "domain-1",
  project_id: "project-1",
  host: "demo.example.com",
  kind: "custom" as const,
  environment: "production" as const,
  status: "active" as const,
  review_status: "approved" as const,
  failure_reason: null,
  is_primary: true,
  host_source_id: null,
  reviewed_by_user_id: "admin-1",
  reviewed_at: "2026-07-30T02:00:00Z",
  review_reason: null,
  created_at: "2026-07-30T02:00:00Z",
  updated_at: "2026-07-30T02:00:00Z",
};

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/admin/projects/project-1"]}>
        <Routes>
          <Route path="/admin/projects/:projectId" element={<ProjectGovernancePage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.mocked(adminApi.getProject).mockResolvedValue({
    project,
    team: { id: "team-1", slug: "acme", name: "Acme" },
  });
  vi.mocked(adminApi.updateProjectSlug).mockResolvedValue({
    project: { id: project.id, uuid: project.uuid, slug: "renamed-site" },
    reason: null,
  });
  vi.mocked(adminApi.listProjectDeployments).mockResolvedValue({ deployments: [deployment] });
  vi.mocked(adminApi.withdrawDeployment).mockResolvedValue({
    deployment: { ...deployment, release_status: "draft", serve_status: "retired" },
    reason: null,
  });
  vi.mocked(adminApi.republishDeployment).mockResolvedValue({
    deployment: deployment,
    release_status: "active",
    release_pending: false,
    review_id: null,
    reason: null,
  });
  vi.mocked(adminApi.listProjectDomains).mockResolvedValue({ domains: [domain] });
  vi.mocked(adminApi.approveDomain).mockResolvedValue({ domain, reason: null });
  vi.mocked(adminApi.rejectDomain).mockResolvedValue({
    domain: { ...domain, status: "disabled", review_status: "rejected" },
    reason: null,
  });
  vi.mocked(adminApi.deleteDomain).mockResolvedValue({ deleted: true, reason: null });
  vi.mocked(adminApi.listProjectActivity).mockResolvedValue({
    events: [
      {
        id: "event-1",
        actor_user_id: "admin-1",
        actor_node_id: null,
        actor_type: "user",
        team_id: "team-1",
        visibility: "platform",
        action: "project.slug_updated",
        target_type: "project",
        target_id: "project-1",
        result: "success",
        reason: "Moderated public identifier",
        metadata: {},
        request_id: null,
        source_ip: null,
        user_agent: null,
        http_method: null,
        request_path: null,
        status_code: null,
        duration_ms: null,
        changes: {},
        created_at: "2026-07-30T03:00:00Z",
      },
    ],
    pagination: { page: 1, per_page: 50, total: 1, total_pages: 1 },
  });
});

it("shows a dedicated project page with read-only configuration and editable slug", async () => {
  renderPage();

  expect(await screen.findByRole("heading", { name: "Demo Site" })).toBeInTheDocument();
  expect(screen.getByText("project-1")).toBeInTheDocument();
  expect(screen.getByText("https://example.com/acme/demo.git")).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Public slug" })).toHaveValue("demo-site");
  expect(screen.queryByRole("textbox", { name: "Project name" })).not.toBeInTheDocument();
  expect(screen.getByRole("tab", { name: "Overview" })).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: "Deployments" })).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: "Domains" })).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: "Activity" })).toBeInTheDocument();
});

it("withdraws a deployment without requiring a governance reason", async () => {
  const user = userEvent.setup();
  renderPage();

  await user.click(await screen.findByRole("tab", { name: "Deployments" }));
  await user.click(await screen.findByRole("button", { name: "Withdraw deployment" }));
  expect(screen.getByText("Reason (optional)")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Confirm withdrawal" }));

  expect(adminApi.withdrawDeployment).toHaveBeenCalledWith("deployment-1", undefined);
});

it("moderates existing domains without exposing a domain creation action", async () => {
  const user = userEvent.setup();
  renderPage();

  await user.click(await screen.findByRole("tab", { name: "Domains" }));
  expect(await screen.findByText("demo.example.com")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /add domain/i })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Reject domain" }));
  await user.click(screen.getByRole("button", { name: "Confirm rejection" }));

  expect(adminApi.rejectDomain).toHaveBeenCalledWith("domain-1", undefined);
});

it("shows project-scoped activity", async () => {
  const user = userEvent.setup();
  renderPage();

  await user.click(await screen.findByRole("tab", { name: "Activity" }));
  expect(await screen.findByText("project.slug_updated")).toBeInTheDocument();
  expect(screen.getByText("Moderated public identifier")).toBeInTheDocument();
  expect(adminApi.listProjectActivity).toHaveBeenCalledWith("project-1", 1);
});
