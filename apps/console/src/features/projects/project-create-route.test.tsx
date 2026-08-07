import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, useLocation } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useTeam } from "@/features/teams/team-context";
import { showErrorToast } from "@/lib/toast";

import { ProjectCreateRoute } from "./project-create-route";
import { projectsApi } from "./projects.api";

vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));
vi.mock("@/lib/toast", () => ({ showErrorToast: vi.fn() }));
vi.mock("./projects.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./projects.api")>();
  return { ...actual, projectsApi: { ...actual.projectsApi, create: vi.fn() } };
});

function LocationProbe() {
  const location = useLocation();
  return <output data-testid="location">{location.pathname}</output>;
}

function renderCreate(role: "member" | "viewer" = "member", isLoading = false) {
  vi.mocked(useTeam).mockReturnValue({
    activeTeam: { id: "team-1", name: "Acme", slug: "acme", role },
    activeRole: role,
    isLoading,
  } as ReturnType<typeof useTeam>);

  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MemoryRouter initialEntries={["/projects/new"]}>
      <QueryClientProvider client={client}>
        <ProjectCreateRoute />
        <LocationProbe />
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(projectsApi.create).mockResolvedValue({
    project: {
      id: "project-1",
      team_id: "team-1",
      slug: "marketing-site",
      name: "Marketing Site",
      runtime: "static",
      repository_url: null,
      default_branch: "main",
      install_command: "npm install",
      build_command: "npm run build",
      output_directory: "dist",
      source_config: { framework_hint: "vite" },
      build_config: {},
      archived_at: null,
      deleted_at: null,
      created_at: "2026-08-01T00:00:00Z",
      updated_at: "2026-08-01T00:00:00Z",
    },
    host_assignment: { assigned: true },
  });
});

describe("ProjectCreateRoute", () => {
  it("renders the full-screen form without a dialog", () => {
    renderCreate();

    expect(screen.getByRole("heading", { name: "Configure Project" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Framework Presets" })).toBeInTheDocument();
    expect(screen.getByLabelText("Project name")).toBeInTheDocument();
    expect(screen.getByLabelText("Slug")).toBeInTheDocument();
    expect(screen.getByLabelText("Git repository URL")).toBeInTheDocument();
    expect(screen.getByLabelText("Default branch")).toHaveValue("main");
    expect(screen.getByRole("button", { name: /Vite/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("generates a slug from the project name until the slug is edited", async () => {
    const user = userEvent.setup();
    renderCreate();

    const name = screen.getByLabelText("Project name");
    const slug = screen.getByLabelText("Slug");
    await user.type(name, "Marketing Site");
    expect(slug).toHaveValue("marketing-site");

    await user.clear(slug);
    await user.type(slug, "custom-site");
    await user.clear(name);
    await user.type(name, "Other Site");
    expect(slug).toHaveValue("custom-site");
  });

  it("submits the selected SSR preset with its build defaults", async () => {
    const user = userEvent.setup();
    renderCreate();

    await user.type(screen.getByLabelText("Project name"), "SSR Site");
    await user.click(screen.getByRole("button", { name: /Next\.js \(SSR\)/ }));
    await user.click(screen.getByRole("button", { name: "Create Project" }));

    await waitFor(() => expect(projectsApi.create).toHaveBeenCalledTimes(1));
    expect(projectsApi.create).toHaveBeenCalledWith({
      team_id: "team-1",
      name: "SSR Site",
      slug: "ssr-site",
      runtime: "ssr",
      repository_url: undefined,
      default_branch: "main",
      install_command: "npm install",
      build_command: "npm run build",
      output_directory: undefined,
      framework_hint: "nextjs-ssr",
    });
  });

  it("accepts scp-style SSH repository URLs", async () => {
    const user = userEvent.setup();
    renderCreate();

    await user.type(screen.getByLabelText("Project name"), "SSH Site");
    await user.type(screen.getByLabelText("Git repository URL"), "git@github.com:acme/site.git");
    await user.click(screen.getByRole("button", { name: "Create Project" }));

    await waitFor(() => expect(projectsApi.create).toHaveBeenCalledTimes(1));
    expect(projectsApi.create).toHaveBeenCalledWith(
      expect.objectContaining({ repository_url: "git@github.com:acme/site.git" }),
    );
  });

  it("keeps the form open while an API error is handled by the global Toast", async () => {
    const user = userEvent.setup();
    vi.mocked(projectsApi.create).mockRejectedValueOnce(new Error("Slug already exists"));
    renderCreate();

    await user.type(screen.getByLabelText("Project name"), "Existing Site");
    await user.click(screen.getByRole("button", { name: "Create Project" }));

    await waitFor(() => expect(projectsApi.create).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Configure Project" })).toBeInTheDocument();
    expect(screen.getByTestId("location")).toHaveTextContent("/projects/new");
  });

  it("navigates to the created project after a successful submit", async () => {
    const user = userEvent.setup();
    renderCreate();

    await user.type(screen.getByLabelText("Project name"), "Marketing Site");
    await user.click(screen.getByRole("button", { name: "Create Project" }));

    await waitFor(() =>
      expect(screen.getByTestId("location")).toHaveTextContent("/projects/project-1"),
    );
  });

  it("shows a Toast access state to viewers", () => {
    renderCreate("viewer");

    expect(showErrorToast).toHaveBeenCalledWith(
      expect.objectContaining({
        message: "You do not have permission to create projects in the active team.",
      }),
      "project-create-forbidden",
    );
    expect(screen.queryByLabelText("Project name")).not.toBeInTheDocument();
  });

  it("waits for team permissions before deciding access", () => {
    vi.mocked(useTeam).mockReturnValue({
      activeTeam: null,
      activeRole: null,
      isLoading: true,
    } as ReturnType<typeof useTeam>);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <MemoryRouter initialEntries={["/projects/new"]}>
        <QueryClientProvider client={client}>
          <ProjectCreateRoute />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(screen.getByText("Loading team permissions...")).toBeInTheDocument();
    expect(screen.queryByLabelText("Project name")).not.toBeInTheDocument();
  });

  it("shows a Toast and retry action when teams fail to load", () => {
    vi.mocked(useTeam).mockReturnValue({
      activeTeam: null,
      activeRole: null,
      isLoading: false,
      error: new Error("Network unavailable"),
      refreshTeams: vi.fn(),
    } as ReturnType<typeof useTeam>);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <MemoryRouter initialEntries={["/projects/new"]}>
        <QueryClientProvider client={client}>
          <ProjectCreateRoute />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(showErrorToast).toHaveBeenCalledWith(
      expect.objectContaining({ message: "Network unavailable" }),
      "project-create-team-error",
    );
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });
});
