import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, it, vi } from "vitest";

import { useTeam } from "@/features/teams/team-context";

import { ProjectsRoute } from "./projects-route";
import { projectsApi } from "./projects.api";

vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));
vi.mock("./projects.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./projects.api")>();
  return { ...actual, projectsApi: { ...actual.projectsApi, list: vi.fn() } };
});

function renderProjects(role: "member" | "viewer") {
  vi.mocked(useTeam).mockReturnValue({
    activeTeam: { id: "team-1", name: "Acme", slug: "acme", role },
    activeRole: role,
  } as ReturnType<typeof useTeam>);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <ProjectsRoute />
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.mocked(projectsApi.list).mockResolvedValue({ projects: [] });
});

describe("ProjectsRoute role capabilities", () => {
  it("hides project creation from viewers", async () => {
    renderProjects("viewer");
    await screen.findByText("No projects yet");
    expect(screen.queryByRole("button", { name: "New project" })).not.toBeInTheDocument();
  });

  it("keeps project creation available to members", async () => {
    renderProjects("member");
    await screen.findByText("No projects yet");
    expect(screen.getByRole("button", { name: "New project" })).toBeInTheDocument();
  });
});
