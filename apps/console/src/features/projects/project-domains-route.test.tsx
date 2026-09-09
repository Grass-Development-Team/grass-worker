import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { ProjectDomainsRoute } from "./project-domains-route";
import { useProject } from "./project-layout";
import { projectsApi } from "./projects.api";

vi.mock("./project-layout", () => ({ useProject: vi.fn() }));
vi.mock("./projects.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./projects.api")>();
  return {
    ...actual,
    projectsApi: {
      ...actual.projectsApi,
      listHosts: vi.fn(),
      verifyHost: vi.fn(),
      renewHostCertificate: vi.fn(),
      updateHostCertificate: vi.fn(),
    },
  };
});

it("restores managed renewal after a custom certificate import", async () => {
  vi.mocked(useProject).mockReturnValue({
    role: "owner",
    project: { id: "project-1", name: "Website" },
  } as ReturnType<typeof useProject>);
  const listed = await projectsApi.listHosts("project-1");
  const certificate = {
    status: "active" as const,
    issuer: "manual" as const,
    regional_issuer: "letsencrypt" as const,
    challenge_method: "http01" as const,
    auto_renew: false,
    issued_at: null,
    expires_at: "2026-12-01T00:00:00Z",
    error: null,
    retry_at: null,
    revision: "manual-1",
    dns_delegation_name: null,
    dns_delegation_target: null,
  };
  vi.mocked(projectsApi.listHosts).mockResolvedValue({
    hosts: [{ ...listed.hosts[0], ownership_status: "verified", certificate }],
  });
  vi.mocked(projectsApi.updateHostCertificate).mockResolvedValue({ certificate });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const user = userEvent.setup();
  render(
    <QueryClientProvider client={client}>
      <ProjectDomainsRoute />
    </QueryClientProvider>,
  );
  await user.click(await screen.findByRole("button", { name: "Use managed certificate" }));
  expect(projectsApi.updateHostCertificate).toHaveBeenCalledWith("project-1", "host-1", {
    certificate_issuer: "letsencrypt",
    certificate_auto_renew: true,
  });
});

it("verifies custom-domain ownership through the domain action", async () => {
  vi.mocked(useProject).mockReturnValue({
    role: "owner",
    project: { id: "project-1", name: "Website" },
  } as ReturnType<typeof useProject>);
  const listed = await projectsApi.listHosts("project-1");
  vi.mocked(projectsApi.verifyHost).mockResolvedValue({ host: listed.hosts[0], verified: true });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const user = userEvent.setup();
  render(
    <QueryClientProvider client={client}>
      <ProjectDomainsRoute />
    </QueryClientProvider>,
  );
  await user.click(await screen.findByRole("button", { name: "Verify domain" }));
  expect(projectsApi.verifyHost).toHaveBeenCalledWith("project-1", "host-1");
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
        region: "default",
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
