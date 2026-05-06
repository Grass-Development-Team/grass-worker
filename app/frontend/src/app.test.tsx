import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { getProjects } from "@/api/projects";
import { routes } from "./router";

function jsonResponse(body: unknown, init: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    ...init,
    headers: { "content-type": "application/json", ...init.headers },
  });
}

function readyInfoResponse() {
  return jsonResponse(
    {
      service: "control-api",
      mode: "ready",
    },
    { status: 200 },
  );
}

function currentUserResponse(
  user: {
    id?: string;
    email?: string;
    is_admin?: boolean;
    is_initial_admin?: boolean;
  } = {},
) {
  return jsonResponse(
    {
      user: {
        id: user.id ?? "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        email: user.email ?? "admin@example.com",
        is_admin: user.is_admin ?? true,
        is_initial_admin: user.is_initial_admin ?? true,
      },
    },
    { status: 200 },
  );
}

type TestProjectStatus = "active" | "archived" | "soft_deleted";
type TestDeploymentStatus = "pending" | "processing" | "ready" | "failed" | "canceled";

type TestProject = {
  id: string;
  owner_user_id?: string;
  slug: string;
  name: string;
  status: TestProjectStatus;
  created_at: string;
  updated_at: string;
  archived_at: string | null;
  soft_deleted_at?: string | null;
};

type TestDeployment = {
  id: string;
  project_id: string;
  status: TestDeploymentStatus;
  source_branch?: string | null;
  source_revision?: string | null;
  created_at: string;
  started_at?: string | null;
  finished_at?: string | null;
};

type TestDeploymentArtifactKind = "static_site" | "build_log";

type TestDeploymentArtifact = {
  id: string;
  deployment_id: string;
  kind: TestDeploymentArtifactKind;
  storage_path: string;
  checksum_sha256?: string | null;
  size_bytes?: number | null;
  created_at: string;
};

type TestRelease = {
  project_id: string;
  project_slug: string;
  primary_host: string | null;
  active_deployment_id?: string | null;
  active_deployment?: TestDeployment | null;
  rollback_deployment_id?: string | null;
};

type TestPlatformHostSource = {
  id: string;
  kind: "wildcard_static" | "dns_managed";
  label: string;
  base_domain: string;
  enabled: boolean;
  allows_auto_assign: boolean;
  created_at: string;
  updated_at: string;
};

type TestProjectHostBinding = {
  id: string;
  project_id: string;
  source_id?: string | null;
  host: string;
  is_primary: boolean;
  created_at: string;
  updated_at: string;
};

type TestUser = {
  id: string;
  email: string;
  is_admin: boolean;
  is_initial_admin: boolean;
  created_at: string;
  updated_at: string;
};

function normalizeProject(project: TestProject) {
  return {
    owner_user_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    soft_deleted_at: null,
    ...project,
  };
}

function projectsResponse(
  projects: TestProject[],
) {
  return jsonResponse(
    {
      projects: projects.map(normalizeProject),
    },
    { status: 200 },
  );
}

function projectResponse(project: TestProject) {
  return jsonResponse(
    {
      project: normalizeProject(project),
    },
    { status: 200 },
  );
}

function normalizeDeployment(deployment: TestDeployment) {
  return {
    source_branch: null,
    source_revision: null,
    started_at: null,
    finished_at: null,
    ...deployment,
  };
}

function deploymentsResponse(deployments: TestDeployment[]) {
  return jsonResponse(
    {
      deployments: deployments.map(normalizeDeployment),
    },
    { status: 200 },
  );
}

function deploymentResponse(deployment: TestDeployment) {
  return jsonResponse(
    {
      deployment: normalizeDeployment(deployment),
    },
    { status: 200 },
  );
}

function normalizeDeploymentArtifact(artifact: TestDeploymentArtifact) {
  return {
    checksum_sha256: null,
    size_bytes: null,
    ...artifact,
  };
}

function deploymentArtifactsResponse(artifacts: TestDeploymentArtifact[]) {
  return jsonResponse(
    {
      artifacts: artifacts.map(normalizeDeploymentArtifact),
    },
    { status: 200 },
  );
}

function deploymentArtifactResponse(artifact: TestDeploymentArtifact) {
  return jsonResponse(
    {
      artifact: normalizeDeploymentArtifact(artifact),
    },
    { status: 201 },
  );
}

function normalizeRelease(release: TestRelease) {
  return {
    active_deployment_id: null,
    active_deployment: null,
    rollback_deployment_id: null,
    ...release,
    active_deployment: release.active_deployment
      ? normalizeDeployment(release.active_deployment)
      : null,
  };
}

function releaseResponse(release: TestRelease) {
  return jsonResponse(
    {
      release: normalizeRelease(release),
    },
    { status: 200 },
  );
}

function usersResponse(users: TestUser[]) {
  return jsonResponse(
    {
      users,
    },
    { status: 200 },
  );
}

function platformHostSourcesResponse(sources: TestPlatformHostSource[]) {
  return jsonResponse(
    {
      sources,
    },
    { status: 200 },
  );
}

function normalizeProjectHostBinding(host: TestProjectHostBinding) {
  return {
    source_id: null,
    ...host,
  };
}

function projectHostsResponse(hosts: TestProjectHostBinding[]) {
  return jsonResponse(
    {
      hosts: hosts.map(normalizeProjectHostBinding),
    },
    { status: 200 },
  );
}

function requestPath(input: string | URL | Request): string {
  if (typeof input === "string") {
    return new URL(input, "http://localhost").pathname;
  }

  if (input instanceof URL) {
    return input.pathname;
  }

  return new URL(input.url, "http://localhost").pathname;
}

function requestUrl(input: string | URL | Request): URL {
  if (typeof input === "string") {
    return new URL(input, "http://localhost");
  }

  if (input instanceof URL) {
    return input;
  }

  return new URL(input.url, "http://localhost");
}

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: 30_000 },
      mutations: { retry: false },
    },
  });
}

function renderRouter(initialEntry: string) {
  const router = createMemoryRouter(routes, {
    initialEntries: [initialEntry],
  });
  const queryClient = createTestQueryClient();

  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );

  return { router, queryClient };
}

describe("auth routing", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: string | URL | Request) => {
        const path = requestPath(input);

        if (path === "/api/v1/info") {
          return readyInfoResponse();
        }

        return jsonResponse({ error: "not authenticated" }, { status: 401 });
      }),
    );
  });

  test("redirects protected routes to login with redirect query", async () => {
    const { router } = renderRouter("/projects?filter=active");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/login");
      expect(router.state.location.search).toBe(
        "?redirect=%2Fprojects%3Ffilter%3Dactive",
      );
    });

    expect(
      await screen.findByRole("button", { name: /sign in/i }),
    ).toBeInTheDocument();
  });

  test("login success redirects to redirect query when present", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return jsonResponse({ error: "not authenticated" }, { status: 401 });
      }

      if (path === "/api/v1/auth/login") {
        return currentUserResponse();
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/login?redirect=%2Fprojects");

    await userEvent.type(
      await screen.findByLabelText(/email/i),
      "admin@example.com",
    );
    await userEvent.type(screen.getByLabelText(/password/i), "secret-pass");
    await userEvent.click(screen.getByRole("button", { name: /sign in/i }));

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
    });
  });

  test("authenticated users visiting login are redirected away", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/login?redirect=%2Fprojects%3Ffilter%3Dactive");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
      expect(router.state.location.search).toBe("?filter=active");
    });
  });

  test("logout returns to login with the current route as redirect", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/projects") {
        return projectsResponse([]);
      }

      if (path === "/api/v1/auth/logout") {
        return new Response(null, { status: 204 });
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/projects");

    await userEvent.click(
      await screen.findByRole("button", { name: /sign out/i }),
    );

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/login");
      expect(router.state.location.search).toBe("?redirect=%2Fprojects");
    });
  });

  test("authenticated root redirects to projects and shows the empty state", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/projects") {
        return projectsResponse([]);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
    });

    expect(await screen.findByText(/no projects yet/i)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /projects/i })).toBeInTheDocument();
  });

  test("authenticated console routes render the console shell", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/projects") {
        return projectsResponse([]);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/projects");

    const navigation = await screen.findByRole("navigation", {
      name: /console navigation/i,
    });

    expect(navigation).toBeInTheDocument();
    expect(navigation).toHaveTextContent("Workspace");
    expect(navigation).toHaveTextContent("Admin");
    expect(screen.getByText("admin@example.com")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /^projects$/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /user settings/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /project management/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /platform host sources/i })).toBeInTheDocument();
  });

  test("non-admin users see workspace navigation without project management", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse({
          is_admin: false,
          is_initial_admin: false,
        });
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/settings");

    expect(await screen.findByRole("heading", { name: /^settings$/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /^projects$/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /user settings/i })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /user settings/i })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(
      screen.getByRole("navigation", { name: /console navigation/i }),
    ).not.toHaveTextContent("Admin");
    expect(
      screen.queryByRole("link", { name: /project management/i }),
    ).not.toBeInTheDocument();
  });

  test("admin users see project management as the current admin navigation entry", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = requestUrl(input);

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted"
      ) {
        return projectsResponse([]);
      }

      throw new Error(`Unexpected request for ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/admin/projects/deleted");

    const projectManagementLink = await screen.findByRole("link", {
      name: /project management/i,
    });

    expect(
      screen.getByRole("navigation", { name: /console navigation/i }),
    ).toHaveTextContent("Admin");
    expect(projectManagementLink).toHaveAttribute("aria-current", "page");
    expect(screen.queryByRole("link", { name: /^admin$/i })).not.toBeInTheDocument();
  });

  test("admin users page loads the admin user inventory", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = requestUrl(input);

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (url.pathname === "/api/v1/admin/users") {
        return usersResponse([
          {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            email: "admin@example.com",
            is_admin: true,
            is_initial_admin: true,
            created_at: "2026-04-17T10:00:00Z",
            updated_at: "2026-04-17T10:00:00Z",
          },
          {
            id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            email: "member@example.com",
            is_admin: false,
            is_initial_admin: false,
            created_at: "2026-04-18T10:00:00Z",
            updated_at: "2026-04-18T10:00:00Z",
          },
        ]);
      }

      throw new Error(`Unexpected request for ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/admin/users");

    expect(await screen.findByRole("heading", { name: /^users$/i })).toBeInTheDocument();
    expect(await screen.findByText("member@example.com")).toBeInTheDocument();
    expect(screen.getAllByText("admin@example.com").length).toBeGreaterThan(0);
  });

  test("admin platform host sources page loads inventory and can disable a source", async () => {
    const sourceId = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let sources: TestPlatformHostSource[] = [
      {
        id: sourceId,
        kind: "wildcard_static",
        label: "Primary Sites",
        base_domain: "apps.example.com",
        enabled: true,
        allows_auto_assign: true,
        created_at: "2026-05-04T10:00:00Z",
        updated_at: "2026-05-04T10:00:00Z",
      },
    ];
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (path === "/api/v1/admin/platform-host-sources" && method === "GET") {
        return platformHostSourcesResponse(sources);
      }

      if (
        path === `/api/v1/admin/platform-host-sources/${sourceId}/disable` &&
        method === "POST"
      ) {
        sources = sources.map((source) =>
          source.id === sourceId ? { ...source, enabled: false } : source,
        );
        return jsonResponse({ source: sources[0] }, { status: 200 });
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/admin/platform-host-sources");

    expect(
      await screen.findByRole("heading", { name: /platform host sources/i }),
    ).toBeInTheDocument();
    expect(await screen.findByText("Primary Sites")).toBeInTheDocument();
    expect(screen.getByText("apps.example.com")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /disable source/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/admin/platform-host-sources/${sourceId}/disable`,
        method: "POST",
        body: undefined,
      });
    });
  });

  test("admin platform host sources page can create a source", async () => {
    const sourceId = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let sources: TestPlatformHostSource[] = [];
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (path === "/api/v1/admin/platform-host-sources" && method === "GET") {
        return platformHostSourcesResponse(sources);
      }

      if (path === "/api/v1/admin/platform-host-sources" && method === "POST") {
        const createdSource: TestPlatformHostSource = {
          id: sourceId,
          kind: body?.kind ?? "wildcard_static",
          label: body?.label ?? "Preview Hosts",
          base_domain: body?.base_domain ?? "preview.example.com",
          enabled: body?.enabled ?? true,
          allows_auto_assign: body?.allows_auto_assign ?? true,
          created_at: "2026-05-05T10:00:00Z",
          updated_at: "2026-05-05T10:00:00Z",
        };
        sources = [createdSource, ...sources];
        return jsonResponse({ source: createdSource }, { status: 201 });
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/admin/platform-host-sources");

    await userEvent.type(await screen.findByLabelText(/label/i), "Preview Hosts");
    await userEvent.type(screen.getByLabelText(/base domain/i), "preview.example.com");
    await userEvent.click(screen.getByRole("button", { name: /create source/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: "/api/v1/admin/platform-host-sources",
        method: "POST",
        body: {
          kind: "wildcard_static",
          label: "Preview Hosts",
          base_domain: "preview.example.com",
          enabled: true,
          allows_auto_assign: true,
        },
      });
    });

    expect(await screen.findByText("Preview Hosts")).toBeInTheDocument();
    expect(screen.getByText("preview.example.com")).toBeInTheDocument();
  });

  test("authenticated users can reach the settings placeholder inside the console", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse({
          is_admin: false,
          is_initial_admin: false,
        });
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/settings");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/settings");
    });

    expect(
      await screen.findByRole("navigation", { name: /console navigation/i }),
    ).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: /^settings$/i })).toBeInTheDocument();
    expect(screen.getByText(/settings are not implemented yet/i)).toBeInTheDocument();
  });

  test("non-admin users are redirected away from admin deleted routes", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse({
          is_admin: false,
          is_initial_admin: false,
        });
      }

      if (path === "/api/v1/projects") {
        return projectsResponse([]);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/admin/projects/deleted");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
    });

    expect(await screen.findByRole("heading", { name: /projects/i })).toBeInTheDocument();
  });

  test("admin project management entry redirects to the deleted-project recovery route", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = requestUrl(input);

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted"
      ) {
        return projectsResponse([]);
      }

      throw new Error(`Unexpected request for ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/admin/projects");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/admin/projects/deleted");
    });

    expect(await screen.findByRole("heading", { name: /project management/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /recovery queue/i })).toBeInTheDocument();
  });

  test("non-admin users are redirected away from unknown admin routes", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse({
          is_admin: false,
          is_initial_admin: false,
        });
      }

      if (path === "/api/v1/projects") {
        return projectsResponse([]);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/admin/unknown");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
    });

    expect(await screen.findByRole("heading", { name: /projects/i })).toBeInTheDocument();
  });

  test("admin deleted projects page fetches only soft-deleted projects", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = requestUrl(input);

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted"
      ) {
        return projectsResponse([
          {
            id: "11111111-1111-1111-1111-111111111111",
            slug: "docs-site",
            name: "Docs Site",
            status: "soft_deleted",
            created_at: "2026-04-20T10:00:00Z",
            updated_at: "2026-04-23T12:00:00Z",
            archived_at: null,
            soft_deleted_at: "2026-04-23T12:00:00Z",
          },
        ]);
      }

      throw new Error(`Unexpected request for ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/admin");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/admin/projects/deleted");
    });

    expect(await screen.findByRole("heading", { name: /project management/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /recovery queue/i })).toBeInTheDocument();
    expect(await screen.findByText("Docs Site")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /restore active/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /restore archived/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /hard delete/i })).toBeInTheDocument();
    expect(
      fetchMock.mock.calls.some(([calledInput]) => {
        const url = requestUrl(calledInput);
        return (
          url.pathname === "/api/v1/projects" &&
          url.searchParams.get("status") === "soft_deleted"
        );
      }),
    ).toBe(true);
  });

  test("admin deleted projects page shows load errors without the empty state", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = requestUrl(input);

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted"
      ) {
        return jsonResponse({ error: "load failed" }, { status: 500 });
      }

      throw new Error(`Unexpected request for ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/admin/projects/deleted");

    expect(await screen.findByText(/unable to load deleted projects/i)).toBeInTheDocument();
    expect(screen.queryByText(/no deleted projects/i)).not.toBeInTheDocument();
  });

  test("projects page renders the authenticated user's project list", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/projects") {
        return projectsResponse([
          {
            id: "11111111-1111-1111-1111-111111111111",
            slug: "docs-site",
            name: "Docs Site",
            status: "active",
            created_at: "2026-04-19T10:00:00Z",
            updated_at: "2026-04-19T10:00:00Z",
            archived_at: null,
          },
          {
            id: "22222222-2222-2222-2222-222222222222",
            slug: "legacy-console",
            name: "Legacy Console",
            status: "archived",
            created_at: "2026-04-18T10:00:00Z",
            updated_at: "2026-04-19T08:00:00Z",
            archived_at: "2026-04-19T08:00:00Z",
          },
        ]);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/projects");

    expect(await screen.findByText(/docs site/i)).toBeInTheDocument();
    expect(screen.getByText(/legacy console/i)).toBeInTheDocument();
    expect(screen.getByText(/docs-site/i)).toBeInTheDocument();
    expect(screen.getByText("Archived project")).toBeInTheDocument();
  });

  test("projects page copy no longer mentions administrator-visible deleted projects", async () => {
    const now = "2026-04-23T12:00:00Z";
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/projects") {
        return projectsResponse([
          {
            id: "11111111-1111-1111-1111-111111111111",
            slug: "docs-site",
            name: "Docs Site",
            status: "active",
            created_at: now,
            updated_at: now,
            archived_at: null,
          },
        ]);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/projects");

    expect(await screen.findByText("Docs Site")).toBeInTheDocument();
    expect(
      screen.getByText("Track active and archived projects for this workspace."),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/administrator-visible soft-deleted projects/i),
    ).not.toBeInTheDocument();
  });

  test("projects page creates a project and refreshes the inventory", async () => {
    let projects: Array<{
      id: string;
      slug: string;
      name: string;
      status: "active" | "archived";
      created_at: string;
      updated_at: string;
      archived_at: string | null;
    }> = [];

    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/projects" && method === "GET") {
        return projectsResponse(projects);
      }

      if (path === "/api/v1/projects" && method === "POST") {
        projects = [
          {
            id: "11111111-1111-1111-1111-111111111111",
            slug: "docs-site",
            name: "Docs Site",
            status: "active",
            created_at: "2026-04-19T10:00:00Z",
            updated_at: "2026-04-19T10:00:00Z",
            archived_at: null,
          },
        ];

        return jsonResponse(
          {
            project: projects[0],
          },
          { status: 201 },
        );
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/projects");

    await userEvent.type(await screen.findByLabelText(/project name/i), "Docs Site");
    await waitFor(() => {
      expect(screen.getByLabelText(/project slug/i)).toHaveValue("docs-site");
    });
    await userEvent.click(screen.getByRole("button", { name: /create project/i }));
    await waitFor(() => {
      expect(
        fetchMock.mock.calls.some(([calledInput, calledInit]) => {
          const method =
            calledInput instanceof Request
              ? calledInput.method
              : ((calledInit as RequestInit | undefined)?.method ?? "GET");

          return requestPath(calledInput) === "/api/v1/projects" && method === "POST";
        }),
      ).toBe(true);
    });
    await waitFor(() => {
      expect(screen.getByLabelText(/project name/i)).toHaveValue("");
    });

    expect(await screen.findByText(/docs site/i)).toBeInTheDocument();
    expect(screen.getByText(/docs-site/i)).toBeInTheDocument();
  });

  test("projects list navigates to the project details page", async () => {
    const project = {
      id: "11111111-1111-1111-1111-111111111111",
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: "2026-04-19T10:00:00Z",
      updated_at: "2026-04-19T10:00:00Z",
      archived_at: null,
    };

    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/projects" && method === "GET") {
        return projectsResponse([project]);
      }

      if (path === `/api/v1/projects/${project.id}` && method === "GET") {
        return projectResponse(project);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/projects");

    await userEvent.click((await screen.findAllByRole("button", { name: /view details/i }))[0]);

    await waitFor(() => {
      expect(router.state.location.pathname).toBe(`/projects/${project.id}`);
    });

    expect(await screen.findByRole("heading", { name: /docs site/i })).toBeInTheDocument();
    expect(screen.getByText(/deployment history/i)).toBeInTheDocument();
  });

  test("project deployment detail route renders deployment metadata", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const deploymentId = "22222222-2222-2222-2222-222222222222";
    const deployment = {
      id: deploymentId,
      project_id: projectId,
      status: "pending" as const,
      source_branch: "main",
      source_revision: "deadbeef",
      created_at: "2026-04-28T12:00:00Z",
      started_at: null,
      finished_at: null,
    };
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}` &&
        method === "GET"
      ) {
        return deploymentResponse(deployment);
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts` &&
        method === "GET"
      ) {
        return deploymentArtifactsResponse([]);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}/deployments/${deploymentId}`);

    expect(
      await screen.findByRole("heading", { name: /deployment details/i }),
    ).toBeInTheDocument();
    expect(screen.getByText("main")).toBeInTheDocument();
    expect(screen.getByText("deadbeef")).toBeInTheDocument();
    expect(screen.getAllByText(/pending/i).length).toBeGreaterThan(0);
  });

  test("project deployment detail route manages lifecycle and deployment artifacts", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const deploymentId = "22222222-2222-2222-2222-222222222222";
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    let deployment: TestDeployment = {
      id: deploymentId,
      project_id: projectId,
      status: "pending",
      source_branch: "main",
      source_revision: "deadbeef",
      created_at: "2026-04-28T12:00:00Z",
      started_at: null,
      finished_at: null,
    };
    const artifacts: TestDeploymentArtifact[] = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}/deployments/${deploymentId}` && method === "GET") {
        return deploymentResponse(deployment);
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts` &&
        method === "GET"
      ) {
        return deploymentArtifactsResponse(artifacts);
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}/transition` &&
        method === "POST"
      ) {
        deployment = {
          ...deployment,
          status: "processing",
          started_at: "2026-04-28T12:05:00Z",
        };
        return deploymentResponse(deployment);
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts` &&
        method === "POST"
      ) {
        const artifact: TestDeploymentArtifact = {
          id: "33333333-3333-3333-3333-333333333333",
          deployment_id: deploymentId,
          kind: (body?.kind ?? "build_log") as TestDeploymentArtifactKind,
          storage_path: body?.storage_path ?? "s3://artifacts/build.log",
          checksum_sha256: body?.checksum_sha256 ?? null,
          size_bytes: body?.size_bytes ?? null,
          created_at: "2026-04-28T12:10:00Z",
        };
        artifacts.unshift(artifact);
        return deploymentArtifactResponse(artifact);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}/deployments/${deploymentId}`);

    expect(
      await screen.findByRole("button", { name: /start processing/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/no artifacts registered yet/i)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /start processing/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/deployments/${deploymentId}/transition`,
        method: "POST",
        body: { status: "processing" },
      });
    });

    expect(await screen.findAllByText(/processing/i)).not.toHaveLength(0);

    await userEvent.selectOptions(screen.getByLabelText(/artifact kind/i), "build_log");
    await userEvent.type(screen.getByLabelText(/storage path/i), "s3://artifacts/build.log");
    await userEvent.type(screen.getByLabelText(/sha256 checksum/i), "abc123");
    await userEvent.type(screen.getByLabelText(/size bytes/i), "1024");
    await userEvent.click(screen.getByRole("button", { name: /register artifact/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts`,
        method: "POST",
        body: {
          kind: "build_log",
          storage_path: "s3://artifacts/build.log",
          checksum_sha256: "abc123",
          size_bytes: 1024,
        },
      });
    });

    expect(await screen.findByText("s3://artifacts/build.log")).toBeInTheDocument();
    expect(screen.getAllByText(/build_log/i).length).toBeGreaterThan(0);
  });

  test("project deployment detail route can activate the live release", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const deploymentId = "22222222-2222-2222-2222-222222222222";
    const deployment: TestDeployment = {
      id: deploymentId,
      project_id: projectId,
      status: "ready",
      source_branch: "main",
      source_revision: "deadbeef",
      created_at: "2026-04-28T12:00:00Z",
      started_at: "2026-04-28T12:05:00Z",
      finished_at: "2026-04-28T12:10:00Z",
    };
    const artifact: TestDeploymentArtifact = {
      id: "33333333-3333-3333-3333-333333333333",
      deployment_id: deploymentId,
      kind: "static_site",
      storage_path: "/tmp/docs-site",
      checksum_sha256: "abc123",
      size_bytes: 1024,
      created_at: "2026-04-28T12:11:00Z",
    };
    let release: TestRelease = {
      project_id: projectId,
      project_slug: "docs-site",
      primary_host: null,
      active_deployment_id: null,
      active_deployment: null,
      rollback_deployment_id: null,
    };
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method = input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}/deployments/${deploymentId}` && method === "GET") {
        return deploymentResponse(deployment);
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts` &&
        method === "GET"
      ) {
        return deploymentArtifactsResponse([artifact]);
      }

      if (path === `/api/v1/projects/${projectId}/release` && method === "GET") {
        return releaseResponse(release);
      }

      if (path === `/api/v1/projects/${projectId}/release/activate` && method === "POST") {
        release = {
          ...release,
          active_deployment_id: deploymentId,
          active_deployment: deployment,
        };
        return releaseResponse(release);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}/deployments/${deploymentId}`);

    expect(
      await screen.findByRole("button", { name: /activate release/i }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /activate release/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/release/activate`,
        method: "POST",
        body: { deployment_id: deploymentId },
      });
    });

    expect(await screen.findByText(/currently live/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /open live site/i })).toBeDisabled();
  });

  test("project deployment detail route renders an error state when lookup fails", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const deploymentId = "22222222-2222-2222-2222-222222222222";
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}` &&
        method === "GET"
      ) {
        return jsonResponse({ error: "deployment not found" }, { status: 404 });
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts` &&
        method === "GET"
      ) {
        return deploymentArtifactsResponse([]);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}/deployments/${deploymentId}`);

    expect(
      await screen.findByRole("heading", { name: /deployment unavailable/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/deployment not found/i)).toBeInTheDocument();
  });

  test("project details loads deployment history and links to deployment details", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const deploymentId = "22222222-2222-2222-2222-222222222222";
    const now = "2026-04-28T12:00:00Z";
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: now,
      updated_at: now,
      archived_at: null,
      soft_deleted_at: null,
    };
    const deployment = {
      id: deploymentId,
      project_id: projectId,
      status: "pending" as const,
      source_branch: "main",
      source_revision: "deadbeef",
      created_at: now,
      started_at: null,
      finished_at: null,
    };
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "GET") {
        return deploymentsResponse([deployment]);
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}` &&
        method === "GET"
      ) {
        return deploymentResponse(deployment);
      }

      if (
        path === `/api/v1/projects/${projectId}/deployments/${deploymentId}/artifacts` &&
        method === "GET"
      ) {
        return deploymentArtifactsResponse([]);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter(`/projects/${projectId}`);

    expect(await screen.findByText("deadbeef")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /view deployment details/i }));

    await waitFor(() => {
      expect(router.state.location.pathname).toBe(
        `/projects/${projectId}/deployments/${deploymentId}`,
      );
    });
  });

  test("project details shows the live release and can roll back", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const currentDeploymentId = "22222222-2222-2222-2222-222222222222";
    const previousDeploymentId = "33333333-3333-3333-3333-333333333333";
    const now = "2026-04-28T12:00:00Z";
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: now,
      updated_at: now,
      archived_at: null,
      soft_deleted_at: null,
    };
    const currentDeployment: TestDeployment = {
      id: currentDeploymentId,
      project_id: projectId,
      status: "ready",
      source_branch: "main",
      source_revision: "current",
      created_at: now,
      started_at: now,
      finished_at: now,
    };
    const previousDeployment: TestDeployment = {
      id: previousDeploymentId,
      project_id: projectId,
      status: "ready",
      source_branch: "main",
      source_revision: "previous",
      created_at: "2026-04-28T11:00:00Z",
      started_at: "2026-04-28T11:00:00Z",
      finished_at: "2026-04-28T11:00:00Z",
    };
    let release: TestRelease = {
      project_id: projectId,
      project_slug: "docs-site",
      primary_host: "docs.example.com",
      active_deployment_id: currentDeploymentId,
      active_deployment: currentDeployment,
      rollback_deployment_id: previousDeploymentId,
    };
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method = input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "GET") {
        return deploymentsResponse([currentDeployment, previousDeployment]);
      }

      if (path === `/api/v1/projects/${projectId}/release` && method === "GET") {
        return releaseResponse(release);
      }

      if (path === `/api/v1/projects/${projectId}/release/rollback` && method === "POST") {
        release = {
          ...release,
          active_deployment_id: previousDeploymentId,
          active_deployment: previousDeployment,
          rollback_deployment_id: currentDeploymentId,
        };
        return releaseResponse(release);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}`);

    expect(await screen.findByRole("link", { name: /open live site/i })).toHaveAttribute(
      "href",
      "https://docs.example.com",
    );
    expect(screen.getByRole("button", { name: /roll back release/i })).toBeInTheDocument();
    expect(screen.getAllByText("current").length).toBeGreaterThan(0);

    await userEvent.click(screen.getByRole("button", { name: /roll back release/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/release/rollback`,
        method: "POST",
        body: undefined,
      });
    });

    expect((await screen.findAllByText("previous")).length).toBeGreaterThan(0);
  });

  test("project details can create a platform host binding and refresh the live site link", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const sourceId = "44444444-4444-4444-4444-444444444444";
    const now = "2026-05-05T12:00:00Z";
    const activeDeployment: TestDeployment = {
      id: "99999999-9999-9999-9999-999999999999",
      project_id: projectId,
      status: "ready",
      source_branch: "main",
      source_revision: "current",
      created_at: now,
      started_at: now,
      finished_at: now,
    };
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: now,
      updated_at: now,
      archived_at: null,
      soft_deleted_at: null,
    };
    let hosts: TestProjectHostBinding[] = [];
    let release: TestRelease = {
      project_id: projectId,
      project_slug: "docs-site",
      primary_host: null,
      active_deployment_id: activeDeployment.id,
      active_deployment: activeDeployment,
      rollback_deployment_id: null,
    };
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "GET") {
        return deploymentsResponse([activeDeployment]);
      }

      if (path === `/api/v1/projects/${projectId}/release` && method === "GET") {
        return releaseResponse(release);
      }

      if (path === `/api/v1/projects/${projectId}/hosts` && method === "GET") {
        return projectHostsResponse(hosts);
      }

      if (path === "/api/v1/admin/platform-host-sources" && method === "GET") {
        return platformHostSourcesResponse([
          {
            id: sourceId,
            kind: "wildcard_static",
            label: "Primary Sites",
            base_domain: "apps.example.com",
            enabled: true,
            allows_auto_assign: true,
            created_at: now,
            updated_at: now,
          },
          {
            id: "55555555-5555-5555-5555-555555555555",
            kind: "wildcard_static",
            label: "Preview Sites",
            base_domain: "preview.example.com",
            enabled: true,
            allows_auto_assign: false,
            created_at: now,
            updated_at: now,
          },
        ]);
      }

      if (path === `/api/v1/projects/${projectId}/hosts` && method === "POST") {
        const createdHost: TestProjectHostBinding = {
          id: "66666666-6666-6666-6666-666666666666",
          project_id: projectId,
          source_id: body?.source_id ?? null,
          host: body?.host ?? "docs.apps.example.com",
          is_primary: body?.is_primary ?? true,
          created_at: now,
          updated_at: now,
        };
        hosts = [createdHost];
        release = {
          ...release,
          primary_host: createdHost.host,
        };
        return jsonResponse({ host: normalizeProjectHostBinding(createdHost) }, { status: 201 });
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}`);

    expect(await screen.findByRole("heading", { name: /host bindings/i })).toBeInTheDocument();

    await userEvent.selectOptions(screen.getByLabelText(/host type/i), "platform_subdomain");
    await userEvent.selectOptions(screen.getByLabelText(/platform source/i), sourceId);
    await userEvent.type(screen.getByLabelText(/subdomain prefix/i), "docs");
    await userEvent.click(screen.getByRole("button", { name: /add host/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/hosts`,
        method: "POST",
        body: {
          host: "docs.apps.example.com",
          source_id: sourceId,
          is_primary: true,
        },
      });
    });

    expect(await screen.findByText("docs.apps.example.com")).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: /open live site/i })).toHaveAttribute(
      "href",
      "https://docs.apps.example.com",
    );
  });

  test("project details can change the primary host and remove bindings", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const primaryHostId = "77777777-7777-7777-7777-777777777777";
    const secondaryHostId = "88888888-8888-8888-8888-888888888888";
    const now = "2026-05-05T12:00:00Z";
    const activeDeployment: TestDeployment = {
      id: "99999999-9999-9999-9999-999999999999",
      project_id: projectId,
      status: "ready",
      source_branch: "main",
      source_revision: "current",
      created_at: now,
      started_at: now,
      finished_at: now,
    };
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: now,
      updated_at: now,
      archived_at: null,
      soft_deleted_at: null,
    };
    let hosts: TestProjectHostBinding[] = [
      {
        id: primaryHostId,
        project_id: projectId,
        source_id: null,
        host: "docs.example.com",
        is_primary: true,
        created_at: now,
        updated_at: now,
      },
      {
        id: secondaryHostId,
        project_id: projectId,
        source_id: null,
        host: "preview.example.com",
        is_primary: false,
        created_at: now,
        updated_at: now,
      },
    ];
    let release: TestRelease = {
      project_id: projectId,
      project_slug: "docs-site",
      primary_host: "docs.example.com",
      active_deployment_id: activeDeployment.id,
      active_deployment: activeDeployment,
      rollback_deployment_id: null,
    };
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "GET") {
        return deploymentsResponse([activeDeployment]);
      }

      if (path === `/api/v1/projects/${projectId}/release` && method === "GET") {
        return releaseResponse(release);
      }

      if (path === `/api/v1/projects/${projectId}/hosts` && method === "GET") {
        return projectHostsResponse(hosts);
      }

      if (
        path === `/api/v1/projects/${projectId}/hosts/${secondaryHostId}/primary` &&
        method === "POST"
      ) {
        hosts = hosts.map((host) => ({
          ...host,
          is_primary: host.id === secondaryHostId,
        }));
        release = {
          ...release,
          primary_host: "preview.example.com",
        };
        return jsonResponse(
          { host: normalizeProjectHostBinding(hosts[1]) },
          { status: 200 },
        );
      }

      if (path === `/api/v1/projects/${projectId}/hosts/${secondaryHostId}` && method === "DELETE") {
        hosts = hosts.filter((host) => host.id !== secondaryHostId);
        release = {
          ...release,
          primary_host: "docs.example.com",
        };
        return new Response(null, { status: 204 });
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}`);

    expect(await screen.findByText("docs.example.com")).toBeInTheDocument();
    expect(await screen.findByText("preview.example.com")).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", {
        name: /set preview\.example\.com as primary/i,
      }),
    );

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/hosts/${secondaryHostId}/primary`,
        method: "POST",
        body: undefined,
      });
    });

    expect(await screen.findByRole("link", { name: /open live site/i })).toHaveAttribute(
      "href",
      "https://preview.example.com",
    );

    await userEvent.click(
      screen.getByRole("button", {
        name: /remove preview\.example\.com/i,
      }),
    );

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/hosts/${secondaryHostId}`,
        method: "DELETE",
        body: undefined,
      });
    });

    await waitFor(() => {
      expect(screen.queryByText("preview.example.com")).not.toBeInTheDocument();
    });
    expect(await screen.findByRole("link", { name: /open live site/i })).toHaveAttribute(
      "href",
      "https://docs.example.com",
    );
  });

  test("project details can create a deployment and prepend it to history", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const createdAt = "2026-04-28T12:10:00Z";
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: "2026-04-28T12:00:00Z",
      updated_at: "2026-04-28T12:00:00Z",
      archived_at: null,
      soft_deleted_at: null,
    };
    const deployments: TestDeployment[] = [];
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "GET") {
        return deploymentsResponse(deployments);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "POST") {
        const created = {
          id: "22222222-2222-2222-2222-222222222222",
          project_id: projectId,
          status: "pending" as const,
          source_branch: body?.source_branch ?? null,
          source_revision: body?.source_revision ?? null,
          created_at: createdAt,
          started_at: null,
          finished_at: null,
        };
        deployments.unshift(created);
        return deploymentResponse(created);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}`);

    await userEvent.type(await screen.findByLabelText(/source branch/i), "main");
    await userEvent.type(screen.getByLabelText(/source revision/i), "deadbeef");
    await userEvent.click(screen.getByRole("button", { name: /create deployment/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/deployments`,
        method: "POST",
        body: {
          source_branch: "main",
          source_revision: "deadbeef",
        },
      });
    });

    expect(await screen.findByText("deadbeef")).toBeInTheDocument();
    expect(screen.getAllByText(/pending/i).length).toBeGreaterThan(0);
  });

  test("project details omit blank deployment source fields and show manual fallbacks", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const createdAt = "2026-04-28T12:10:00Z";
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: "2026-04-28T12:00:00Z",
      updated_at: "2026-04-28T12:00:00Z",
      archived_at: null,
      soft_deleted_at: null,
    };
    const deployments: TestDeployment[] = [];
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "GET") {
        return deploymentsResponse(deployments);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "POST") {
        const created = {
          id: "33333333-3333-3333-3333-333333333333",
          project_id: projectId,
          status: "pending" as const,
          source_branch: body?.source_branch ?? null,
          source_revision: body?.source_revision ?? null,
          created_at: createdAt,
          started_at: null,
          finished_at: null,
        };
        deployments.unshift(created);
        return deploymentResponse(created);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}`);

    await userEvent.click(await screen.findByRole("button", { name: /create deployment/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/deployments`,
        method: "POST",
        body: {},
      });
    });

    expect(await screen.findByText("Manual deployment")).toBeInTheDocument();
    expect(screen.getByText("not set")).toBeInTheDocument();
  });

  test("archived projects disable deployment creation", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const now = "2026-04-28T12:00:00Z";
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "archived" as const,
      created_at: now,
      updated_at: now,
      archived_at: now,
      soft_deleted_at: null,
    };
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/deployments` && method === "GET") {
        return deploymentsResponse([]);
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}`);

    expect(
      await screen.findByText(/archived projects cannot create new deployments/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /create deployment/i })).toBeDisabled();
  });

  test("workspace project details use delete wording and hide hard delete", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const now = "2026-04-23T12:00:00Z";
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}`) {
        return projectResponse({
          id: projectId,
          slug: "docs-site",
          name: "Docs Site",
          status: "active",
          created_at: now,
          updated_at: now,
          archived_at: null,
          soft_deleted_at: null,
        });
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}`);

    expect(await screen.findByRole("heading", { name: /docs site/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /overview/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /edit project/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /lifecycle/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /transfer owner/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /danger zone/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /delete project/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /hard delete/i })).not.toBeInTheDocument();
    expect(screen.queryByText(/soft delete project/i)).not.toBeInTheDocument();
  });

  test("deleting a project soft-deletes it and returns to the projects list without it", async () => {
    const requests: Array<{ path: string; method: string }> = [];
    const projectId = "11111111-1111-1111-1111-111111111111";
    const now = "2026-04-23T12:00:00Z";
    const activeProjects: TestProject[] = [
      {
        id: projectId,
        slug: "docs-site",
        name: "Docs Site",
        status: "active" as const,
        created_at: now,
        updated_at: now,
        archived_at: null,
        soft_deleted_at: null,
      },
      {
        id: "22222222-2222-2222-2222-222222222222",
        slug: "marketing-site",
        name: "Marketing Site",
        status: "active" as const,
        created_at: now,
        updated_at: now,
        archived_at: null,
        soft_deleted_at: null,
      },
    ];
    const project = activeProjects[0];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      requests.push({ path, method });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/projects" && method === "GET") {
        return projectsResponse(activeProjects);
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/soft-delete` && method === "POST") {
        activeProjects.splice(
          activeProjects.findIndex((item) => item.id === projectId),
          1,
        );
        return projectResponse({
          ...project,
          status: "soft_deleted",
          soft_deleted_at: now,
        });
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/projects");

    await userEvent.click((await screen.findAllByRole("button", { name: /view details/i }))[0]);

    expect(await screen.findByRole("button", { name: /delete project/i })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /delete project/i }));
    await userEvent.click(screen.getByRole("button", { name: /^delete project$/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/soft-delete`,
        method: "POST",
      });
    });
    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
    });

    expect(await screen.findByText("Marketing Site")).toBeInTheDocument();
    expect(screen.queryByText("Docs Site")).not.toBeInTheDocument();
  });

  test("admin deleted projects page restores from the admin area", async () => {
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const projectId = "11111111-1111-1111-1111-111111111111";
    const deletedProjects = [
      {
        id: projectId,
        slug: "docs-site",
        name: "Docs Site",
        status: "soft_deleted" as const,
        created_at: "2026-04-20T10:00:00Z",
        updated_at: "2026-04-23T12:00:00Z",
        archived_at: null,
        soft_deleted_at: "2026-04-23T12:00:00Z",
      },
    ];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = requestUrl(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path: `${url.pathname}${url.search}`, method, body });

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted" &&
        method === "GET"
      ) {
        return projectsResponse(deletedProjects);
      }

      if (url.pathname === `/api/v1/projects/${projectId}/restore` && method === "POST") {
        deletedProjects.splice(0, 1);

        return projectResponse({
          id: projectId,
          slug: "docs-site",
          name: "Docs Site",
          status: "active",
          created_at: "2026-04-20T10:00:00Z",
          updated_at: "2026-04-23T12:10:00Z",
          archived_at: null,
          soft_deleted_at: null,
        });
      }

      throw new Error(`Unexpected request for ${method} ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/admin/projects/deleted");

    await userEvent.click(await screen.findByRole("button", { name: /restore active/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/restore`,
        method: "POST",
        body: { status: "active" },
      });
    });

    expect(await screen.findByText(/no deleted projects/i)).toBeInTheDocument();
    expect(screen.queryByText("Docs Site")).not.toBeInTheDocument();
  });

  test("admin deleted projects page restores archived from the admin area", async () => {
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const projectId = "11111111-1111-1111-1111-111111111111";
    const deletedProjects = [
      {
        id: projectId,
        slug: "docs-site",
        name: "Docs Site",
        status: "soft_deleted" as const,
        created_at: "2026-04-20T10:00:00Z",
        updated_at: "2026-04-23T12:00:00Z",
        archived_at: null,
        soft_deleted_at: "2026-04-23T12:00:00Z",
      },
    ];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = requestUrl(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path: `${url.pathname}${url.search}`, method, body });

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted" &&
        method === "GET"
      ) {
        return projectsResponse(deletedProjects);
      }

      if (url.pathname === `/api/v1/projects/${projectId}/restore` && method === "POST") {
        deletedProjects.splice(0, 1);

        return projectResponse({
          id: projectId,
          slug: "docs-site",
          name: "Docs Site",
          status: "archived",
          created_at: "2026-04-20T10:00:00Z",
          updated_at: "2026-04-23T12:10:00Z",
          archived_at: "2026-04-23T12:10:00Z",
          soft_deleted_at: null,
        });
      }

      throw new Error(`Unexpected request for ${method} ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/admin/projects/deleted");

    await userEvent.click(await screen.findByRole("button", { name: /restore archived/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/restore`,
        method: "POST",
        body: { status: "archived" },
      });
    });

    expect(await screen.findByText(/no deleted projects/i)).toBeInTheDocument();
    expect(screen.queryByText("Docs Site")).not.toBeInTheDocument();
  });

  test("admin deleted projects page hard-deletes from the admin area", async () => {
    const requests: Array<{ path: string; method: string }> = [];
    const projectId = "11111111-1111-1111-1111-111111111111";
    const deletedProjects = [
      {
        id: projectId,
        slug: "docs-site",
        name: "Docs Site",
        status: "soft_deleted" as const,
        created_at: "2026-04-20T10:00:00Z",
        updated_at: "2026-04-23T12:00:00Z",
        archived_at: null,
        soft_deleted_at: "2026-04-23T12:00:00Z",
      },
    ];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = requestUrl(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      requests.push({ path: `${url.pathname}${url.search}`, method });

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted" &&
        method === "GET"
      ) {
        return projectsResponse(deletedProjects);
      }

      if (url.pathname === `/api/v1/projects/${projectId}/hard-delete` && method === "POST") {
        deletedProjects.splice(0, 1);
        return new Response(null, { status: 204 });
      }

      throw new Error(`Unexpected request for ${method} ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/admin/projects/deleted");

    await userEvent.click(await screen.findByRole("button", { name: /hard delete/i }));
    await userEvent.click(screen.getByRole("button", { name: /^hard delete$/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/hard-delete`,
        method: "POST",
      });
    });

    expect(await screen.findByText(/no deleted projects/i)).toBeInTheDocument();
    expect(screen.queryByText("Docs Site")).not.toBeInTheDocument();
  });

  test("soft-deleting from project details refreshes the admin project-management queue", async () => {
    const projectId = "11111111-1111-1111-1111-111111111111";
    const now = "2026-04-23T12:00:00Z";
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: now,
      updated_at: now,
      archived_at: null,
      soft_deleted_at: null,
    };
    const activeProjects = [
      project,
    ];
    const deletedProjects: TestProject[] = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = requestUrl(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");

      if (url.pathname === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (url.pathname === "/api/v1/me") {
        return currentUserResponse({ is_admin: true });
      }

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted" &&
        method === "GET"
      ) {
        return projectsResponse(deletedProjects);
      }

      if (url.pathname === "/api/v1/projects" && !url.search && method === "GET") {
        return projectsResponse(activeProjects);
      }

      if (url.pathname === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (url.pathname === `/api/v1/projects/${projectId}/soft-delete` && method === "POST") {
        activeProjects.splice(0, 1);
        const deletedProject = {
          ...project,
          status: "soft_deleted" as const,
          updated_at: "2026-04-23T12:05:00Z",
          soft_deleted_at: "2026-04-23T12:05:00Z",
        };
        deletedProjects.push(deletedProject);
        return projectResponse(deletedProject);
      }

      throw new Error(`Unexpected request for ${method} ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/admin/projects/deleted");

    expect(await screen.findByText(/no deleted projects/i)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("link", { name: /^projects$/i }));
    await userEvent.click(await screen.findByRole("button", { name: /view details/i }));
    await userEvent.click(await screen.findByRole("button", { name: /delete project/i }));
    await userEvent.click(screen.getByRole("button", { name: /^delete project$/i }));

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
    });

    await userEvent.click(screen.getByRole("link", { name: /project management/i }));

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/admin/projects/deleted");
    });
    expect(await screen.findByRole("heading", { name: /project management/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /recovery queue/i })).toBeInTheDocument();
    expect(screen.getByText("Docs Site")).toBeInTheDocument();
  });

  test("project detail actions call backend action endpoints", async () => {
    const requests: Array<{ path: string; method: string; body?: unknown }> = [];
    const projectId = "11111111-1111-1111-1111-111111111111";
    const now = "2026-04-23T12:00:00Z";
    const project = {
      id: projectId,
      slug: "docs-site",
      name: "Docs Site",
      status: "active" as const,
      created_at: now,
      updated_at: now,
      archived_at: null,
      soft_deleted_at: null,
    };
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const path = requestPath(input);
      const method =
        input instanceof Request ? input.method : (init?.method ?? "GET");
      const body = init?.body ? JSON.parse(init.body.toString()) : undefined;
      requests.push({ path, method, body });

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === `/api/v1/projects/${projectId}` && method === "GET") {
        return projectResponse(project);
      }

      if (path === `/api/v1/projects/${projectId}/archive`) {
        return projectResponse({
          ...project,
          status: "archived",
          archived_at: now,
        });
      }

      throw new Error(`Unexpected request for ${method} ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter(`/projects/${projectId}`);

    await userEvent.click(await screen.findByRole("button", { name: /archive project/i }));
    await userEvent.click(screen.getByRole("button", { name: /^archive$/i }));

    await waitFor(() => {
      expect(requests).toContainEqual({
        path: `/api/v1/projects/${projectId}/archive`,
        method: "POST",
        body: undefined,
      });
    });
  });
});

describe("projects api", () => {
  test("getProjects requests soft-deleted projects when asked", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = requestUrl(input);

      if (
        url.pathname === "/api/v1/projects" &&
        url.searchParams.get("status") === "soft_deleted"
      ) {
        return projectsResponse([]);
      }

      throw new Error(`Unexpected request for ${url.pathname}${url.search}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    await expect(getProjects({ status: "soft_deleted" })).resolves.toEqual([]);
  });
});

describe("setup routing", () => {
  test("redirects protected routes to setup when system mode is setup", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return jsonResponse(
          {
            service: "control-api",
            mode: "setup",
            stage: "database",
            status: "pending",
          },
          { status: 200 },
        );
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/projects");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/setup");
    });

    expect(await screen.findByLabelText(/host/i)).toBeInTheDocument();
  });

  test("redirects login route to setup when system mode is setup", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return jsonResponse(
          {
            service: "control-api",
            mode: "setup",
            stage: "admin",
            status: "pending",
          },
          { status: 200 },
        );
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/login");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/setup");
    });

    expect(await screen.findByLabelText(/confirm password/i)).toBeInTheDocument();
  });

  test("redirects setup route to the ready application when setup is complete", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return jsonResponse(
          {
            service: "control-api",
            mode: "ready",
          },
          { status: 200 },
        );
      }

      if (path === "/api/v1/me") {
        return jsonResponse(
          {
            user: {
              id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
              email: "admin@example.com",
              is_admin: true,
              is_initial_admin: true,
            },
          },
          { status: 200 },
        );
      }

      if (path === "/api/v1/projects") {
        return projectsResponse([]);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/setup");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
    });

    expect(await screen.findByText(/no projects yet/i)).toBeInTheDocument();
  });

  test("successful database setup advances to the admin stage", async () => {
    let stage: "database" | "admin" = "database";

    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return jsonResponse(
          {
            service: "control-api",
            mode: "setup",
            stage,
            status: "pending",
          },
          { status: 200 },
        );
      }

      if (path === "/api/v1/setup/database") {
        stage = "admin";
        return jsonResponse(
          {
            stage: "database",
            status: "completed",
          },
          { status: 200 },
        );
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/setup");

    await userEvent.clear(await screen.findByLabelText(/database name/i));
    await userEvent.type(screen.getByLabelText(/database name/i), "grass");
    await userEvent.type(screen.getByLabelText(/username/i), "postgres");
    await userEvent.type(screen.getByLabelText(/^password$/i), "secret-pass");
    await userEvent.click(screen.getByRole("button", { name: /save and continue/i }));

    expect(await screen.findByLabelText(/confirm password/i)).toBeInTheDocument();
  });

  test("admin setup blocks submission when passwords do not match", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return jsonResponse(
          {
            service: "control-api",
            mode: "setup",
            stage: "admin",
            status: "pending",
          },
          { status: 200 },
        );
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/setup");

    await userEvent.type(await screen.findByLabelText(/email/i), "admin@example.com");
    await userEvent.type(screen.getByLabelText(/^password$/i), "secret-pass");
    await userEvent.type(screen.getByLabelText(/confirm password/i), "different-pass");
    await userEvent.click(screen.getByRole("button", { name: /finish setup/i }));

    expect(await screen.findByText(/passwords do not match/i)).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  test("admin setup redirects to login with the created email when setup reaches ready mode", async () => {
    let systemInfo: { service: string; mode: "setup"; stage: "admin"; status: "pending" } | {
      service: string;
      mode: "ready";
    } = {
      service: "control-api",
      mode: "setup",
      stage: "admin",
      status: "pending",
    };

    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return jsonResponse(systemInfo, { status: 200 });
      }

      if (path === "/api/v1/setup/admin") {
        systemInfo = {
          service: "control-api",
          mode: "ready",
        };

        return jsonResponse(
          {
            stage: "admin",
            status: "completed",
          },
          { status: 200 },
        );
      }

      if (path === "/api/v1/me") {
        return jsonResponse({ error: "not authenticated" }, { status: 401 });
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/setup");

    await userEvent.type(await screen.findByLabelText(/email/i), "admin@example.com");
    await userEvent.type(screen.getByLabelText(/^password$/i), "secret-pass");
    await userEvent.type(screen.getByLabelText(/confirm password/i), "secret-pass");
    await userEvent.click(screen.getByRole("button", { name: /finish setup/i }));

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/login");
      expect(router.state.location.search).toBe(
        "?redirect=%2Fprojects&email=admin%40example.com",
      );
    });

    expect(await screen.findByLabelText(/email/i)).toHaveValue("admin@example.com");
  });
});
