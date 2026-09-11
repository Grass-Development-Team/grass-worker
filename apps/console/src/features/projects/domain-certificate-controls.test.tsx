import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { DomainCertificateControls } from "./domain-certificate-controls";
import { projectsApi, type DomainCertificate, type ProjectHost } from "./projects.api";
const certificate: DomainCertificate = {
  issuer: "letsencrypt",
  platform_issuer: "letsencrypt",
  challenge_method: "http01",
  status: "active",
  auto_renew: true,
  issued_at: "2026-09-11T00:00:00Z",
  expires_at: "2026-12-01T00:00:00Z",
  error: null,
  retry_at: null,
  revision: "r1",
  https_ready: false,
  installed_nodes: 1,
  required_nodes: 2,
};
function setup(overrides: Partial<DomainCertificate> = {}) {
  const host = {
    id: "host-1",
    host: "site.example.org",
    kind: "custom",
    ownership_status: "verified",
    certificate: { ...certificate, ...overrides },
  } as ProjectHost;
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DomainCertificateControls host={host} projectId="project-1" canEdit onChange={vi.fn()} />
    </QueryClientProvider>,
  );
}
afterEach(() => vi.restoreAllMocks());
it("waits for entry installation before claiming HTTPS is ready", () => {
  setup();
  expect(screen.getByText("Installing certificate")).toBeInTheDocument();
  expect(screen.queryByText("HTTPS ready")).not.toBeInTheDocument();
});
it("keeps valid HTTPS visible when renewal fails and displays the next retry", () => {
  setup({
    status: "failed",
    https_ready: true,
    error: "Temporary authority failure",
    retry_at: "2026-09-12T00:00:00Z",
  });
  expect(screen.getByText("HTTPS ready")).toBeInTheDocument();
  expect(screen.getByText("Temporary authority failure")).toBeInTheDocument();
  expect(screen.getByText(/Next attempt/)).toBeInTheDocument();
});
it("defaults renewal on and allows changing it in certificate settings", async () => {
  vi.spyOn(projectsApi, "updateHostCertificate").mockResolvedValue({ certificate });
  setup();
  const user = userEvent.setup();
  await user.click(screen.getByText("Certificate settings"));
  expect(screen.getByRole("checkbox", { name: "Automatic renewal" })).toBeChecked();
  await user.click(screen.getByRole("checkbox", { name: "Automatic renewal" }));
  await waitFor(() =>
    expect(projectsApi.updateHostCertificate).toHaveBeenCalledWith("project-1", "host-1", {
      certificate_auto_renew: false,
    }),
  );
});
