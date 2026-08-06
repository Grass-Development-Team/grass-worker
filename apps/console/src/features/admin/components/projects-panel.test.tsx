import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { adminApi, type AdminProject } from "../admin.api";
import { ProjectsPanel } from "./projects-panel";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      listProjects: vi.fn(),
      batchProjects: vi.fn(),
      restoreProject: vi.fn(),
    },
  };
});

const projects: AdminProject[] = [
  {
    id: "project-1",
    slug: "active-project",
    name: "Active project",
    runtime: "static",
    repository_url: null,
    team: { id: "team-1", slug: "team", name: "Team" },
    latest_deployment: null,
    status: "active",
    archived_at: null,
    deleted_at: null,
    created_at: "2026-08-04T00:00:00Z",
  },
  {
    id: "project-2",
    slug: "deleted-project",
    name: "Deleted project",
    runtime: "static",
    repository_url: null,
    team: { id: "team-1", slug: "team", name: "Team" },
    latest_deployment: null,
    status: "deleted",
    archived_at: null,
    deleted_at: "2026-08-06T00:00:00Z",
    created_at: "2026-08-04T00:00:00Z",
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(adminApi.listProjects).mockResolvedValue({ projects });
  vi.mocked(adminApi.batchProjects).mockResolvedValue({
    results: [{ id: "project-1", success: true }],
  });
  vi.mocked(adminApi.restoreProject).mockResolvedValue({ project: projects[0] });
});

it("filters, batches, and restores projects from the table", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const user = userEvent.setup();
  render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <ProjectsPanel />
      </QueryClientProvider>
    </MemoryRouter>,
  );

  await screen.findByText("Deleted project");
  await user.click(screen.getByRole("combobox", { name: "Project status" }));
  await user.click(screen.getByRole("option", { name: "Active" }));
  await waitFor(() => expect(adminApi.listProjects).toHaveBeenLastCalledWith({ status: "active" }));

  await user.click(screen.getByRole("checkbox", { name: "Select Active project" }));
  await user.click(screen.getByRole("button", { name: "Bulk actions" }));
  await user.click(screen.getByRole("menuitem", { name: "Archive selected" }));
  await waitFor(() =>
    expect(adminApi.batchProjects).toHaveBeenCalledWith({
      action: "archive",
      ids: ["project-1"],
    }),
  );

  await user.click(screen.getByRole("button", { name: "Restore Deleted project" }));
  await waitFor(() => expect(adminApi.restoreProject).toHaveBeenCalledWith("project-2"));
});
