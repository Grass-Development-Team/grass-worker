import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

import { ProjectDomainsRoute } from "./project-domains-route";
import { useProject } from "./project-layout";
import { projectsApi } from "./projects.api";
import { regionsApi } from "@/features/regions/regions.api";
afterEach(() => vi.restoreAllMocks());

vi.mock("./project-layout", () => ({ useProject: vi.fn() }));
vi.mock("./projects.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./projects.api")>();
  return {
    ...actual,
    projectsApi: {
      ...actual.projectsApi,
      listHosts: vi.fn(),
      createHost: vi.fn(),
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
    platform_issuer: "letsencrypt" as const,
    challenge_method: "http01" as const,
    auto_renew: false,
    issued_at: null,
    expires_at: "2026-12-01T00:00:00Z",
    error: null,
    retry_at: null,
    revision: "manual-1",
    https_ready: false,
    installed_nodes: 0,
    required_nodes: 1,
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
  await user.click(await screen.findByText("Certificate settings"));
  await user.click(screen.getByRole("button", { name: "Use managed certificate" }));
  expect(projectsApi.updateHostCertificate).toHaveBeenCalledWith("project-1", "host-1", {
    certificate_issuer: "letsencrypt",
    certificate_auto_renew: true,
  });
});

it("checks DNS and ownership through the domain action", async () => {
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
  await user.click(await screen.findByRole("button", { name: "Check now" }));
  expect(projectsApi.verifyHost).toHaveBeenCalledWith("project-1", "host-1");
});

beforeEach(() => {
  vi.spyOn(regionsApi, "list").mockResolvedValue({
    regions: [
      {
        code: "hk_1",
        name: "hk_1",
        ingress_hostname: "hk.entry.example.com",
        ingress_enabled: true,
      },
      { code: "hk_frick", name: "hk_frick", ingress_hostname: null, ingress_enabled: false },
    ],
  });
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

it("requires an enabled regional entry and shows automatic DNS checks after adding a domain", async () => {
  vi.mocked(useProject).mockReturnValue({
    role: "owner",
    project: { id: "project-1", name: "Website" },
  } as ReturnType<typeof useProject>);
  const listed = await projectsApi.listHosts("project-1");
  const host = {
    ...listed.hosts[0],
    region: "hk_1",
    status: "pending" as const,
    connection_state: "unresolved",
    failure_reason: null,
    onboarding: {
      dns_status: "unresolved",
      dns_error: "Add the displayed CNAME record",
      checked_at: "2026-09-11T00:00:00Z",
      next_check_at: "2026-09-11T00:01:30Z",
    },
  };
  vi.mocked(projectsApi.listHosts).mockResolvedValue({ hosts: [] });
  vi.mocked(projectsApi.createHost).mockImplementation(async () => {
    vi.mocked(projectsApi.listHosts).mockResolvedValue({ hosts: [host] });
    return { host };
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <ProjectDomainsRoute />
    </QueryClientProvider>,
  );
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("Add domain"), "www.example.com");
  expect(screen.getByRole("button", { name: "Add" })).toBeDisabled();
  await waitFor(() => expect(screen.getByLabelText("Region")).toBeEnabled());
  await user.click(screen.getByLabelText("Region"));
  expect(screen.getByRole("option", { name: /hk_frick/ })).toHaveAttribute("aria-disabled", "true");
  await user.click(screen.getByRole("option", { name: "hk_1" }));
  await user.click(screen.getByRole("button", { name: "Add" }));
  await waitFor(() =>
    expect(projectsApi.createHost).toHaveBeenCalledWith("project-1", {
      host: "www.example.com",
      region: "hk_1",
    }),
  );
  expect(await screen.findByText("Add the displayed CNAME record")).toBeInTheDocument();
  expect(screen.getByText(/Next check/)).toBeInTheDocument();
});
