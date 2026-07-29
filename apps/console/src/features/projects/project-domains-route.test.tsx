import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { ProjectDomainsRoute } from "./project-domains-route";
import { useProject } from "./project-layout";
import { projectsApi } from "./projects.api";

vi.mock("./project-layout", () => ({ useProject: vi.fn() }));
vi.mock("./projects.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./projects.api")>();
  return {
    ...actual,
    projectsApi: { ...actual.projectsApi, listHosts: vi.fn() },
  };
});

beforeEach(() => {
  vi.mocked(useProject).mockReturnValue({
    role: "viewer",
    project: { id: "project-1", name: "Website" },
  } as ReturnType<typeof useProject>);
  vi.mocked(projectsApi.listHosts).mockResolvedValue({
    hosts: [
      {
        id: "host-1",
        project_id: "project-1",
        host: "www.example.com",
        kind: "custom",
        environment: "production",
        status: "failed",
        failure_reason: "DNS record missing",
        is_primary: false,
        host_source_id: "source-1",
        created_at: "2026-07-29T00:00:00Z",
      },
    ],
  });
});

it("shows domains read-only to viewers", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ProjectDomainsRoute />
    </QueryClientProvider>,
  );

  expect(await screen.findByText("www.example.com")).toBeInTheDocument();
  expect(screen.getByText("DNS record missing")).toBeInTheDocument();
  expect(screen.queryByLabelText("Add domain")).not.toBeInTheDocument();
  expect(screen.queryByRole("columnheader", { name: "Actions" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Make primary" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Remove www.example.com" })).not.toBeInTheDocument();
});
