import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import {
  credentialMatchesRepository,
  ProjectSettingsBuildRoute,
} from "./project-settings-build-route";
import { useProject } from "./project-layout";
import { projectsApi } from "./projects.api";

vi.mock("./project-layout", () => ({ useProject: vi.fn() }));
vi.mock("./projects.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./projects.api")>();
  return {
    ...actual,
    projectsApi: { ...actual.projectsApi, getSourceCredential: vi.fn() },
  };
});

beforeEach(() => {
  vi.mocked(projectsApi.getSourceCredential).mockResolvedValue({ credential: null });
});

it("shows build settings read-only to viewers", async () => {
  vi.mocked(useProject).mockReturnValue({
    role: "viewer",
    project: {
      id: "project-1",
      team_id: "team-1",
      repository_url: "https://example.com/repository.git",
      default_branch: "main",
      source_config: { root_directory: "." },
      install_command: "npm install",
      build_command: "npm run build",
      output_directory: "dist",
    },
  } as ReturnType<typeof useProject>);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={client}>
      <ProjectSettingsBuildRoute />
    </QueryClientProvider>,
  );

  await screen.findByText("Anonymous access");
  for (const input of screen.getAllByRole("textbox")) {
    expect(input).toHaveAttribute("readonly");
  }
  expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Save binding" })).not.toBeInTheDocument();
});

describe("credentialMatchesRepository", () => {
  it("matches only scheme, normalized host, and effective port", () => {
    expect(
      credentialMatchesRepository(
        { kind: "https", host: "example.com", port: 443 },
        "https://EXAMPLE.com/org/repo.git",
      ),
    ).toBe(true);
    expect(
      credentialMatchesRepository(
        { kind: "https", host: "example.com", port: 443 },
        "https://example.com:8443/org/repo.git",
      ),
    ).toBe(false);
    expect(
      credentialMatchesRepository(
        { kind: "ssh", host: "example.com", port: 22 },
        "git@example.com:org/repo.git",
      ),
    ).toBe(true);
    expect(
      credentialMatchesRepository(
        { kind: "ssh", host: "example.com", port: 2222 },
        "ssh://git@example.com:2222/org/repo.git",
      ),
    ).toBe(true);
  });
});
