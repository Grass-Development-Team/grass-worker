import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, test, vi } from "vitest";
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

function requestPath(input: string | URL | Request): string {
  if (typeof input === "string") {
    return new URL(input, "http://localhost").pathname;
  }

  if (input instanceof URL) {
    return input.pathname;
  }

  return new URL(input.url, "http://localhost").pathname;
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

    expect(
      await screen.findByRole("navigation", { name: /console navigation/i }),
    ).toBeInTheDocument();
    expect(screen.getByText("admin@example.com")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /projects/i })).toBeInTheDocument();
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
    expect(screen.getByText(/archived project/i)).toBeInTheDocument();
  });

  test("projects page shows mixed project statuses in one console list", async () => {
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
          {
            id: "22222222-2222-2222-2222-222222222222",
            slug: "old-site",
            name: "Old Site",
            status: "soft_deleted",
            created_at: now,
            updated_at: now,
            archived_at: null,
            soft_deleted_at: now,
          },
        ]);
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    renderRouter("/projects");

    expect(await screen.findByText("Docs Site")).toBeInTheDocument();
    expect(screen.getByText("Old Site")).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.getByText("Soft deleted")).toBeInTheDocument();
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

    await userEvent.click(await screen.findByRole("button", { name: /view details/i }));

    await waitFor(() => {
      expect(router.state.location.pathname).toBe(`/projects/${project.id}`);
    });

    expect(await screen.findByRole("heading", { name: /docs site/i })).toBeInTheDocument();
    expect(screen.getByText(/deployment history/i)).toBeInTheDocument();
  });

  test("admin project details show management sections and hard delete", async () => {
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
          status: "soft_deleted",
          created_at: now,
          updated_at: now,
          archived_at: null,
          soft_deleted_at: now,
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
    expect(screen.getByRole("button", { name: /hard delete/i })).toBeInTheDocument();
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
