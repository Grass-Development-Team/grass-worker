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

function currentUserResponse() {
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
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

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
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

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

  test("logout returns to login with redirect root", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const path = requestPath(input);

      if (path === "/api/v1/info") {
        return readyInfoResponse();
      }

      if (path === "/api/v1/me") {
        return currentUserResponse();
      }

      if (path === "/api/v1/auth/logout") {
        return new Response(null, { status: 204 });
      }

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/");

    await userEvent.click(
      await screen.findByRole("button", { name: /sign out/i }),
    );

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/login");
      expect(router.state.location.search).toBe("?redirect=%2F");
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

      throw new Error(`Unexpected request for ${path}`);
    });

    vi.stubGlobal("fetch", fetchMock);

    const { router } = renderRouter("/setup");

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/");
    });

    expect(await screen.findByText(/ready mode is active/i)).toBeInTheDocument();
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
});
