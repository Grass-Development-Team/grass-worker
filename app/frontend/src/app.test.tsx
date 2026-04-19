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

describe("auth routing", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        jsonResponse({ error: "not authenticated" }, { status: 401 }),
      ),
    );
  });

  test("redirects protected routes to login with redirect query", async () => {
    const router = createMemoryRouter(routes, {
      initialEntries: ["/projects?filter=active"],
    });
    const queryClient = new QueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );

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
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse({ error: "not authenticated" }, { status: 401 }),
      )
      .mockResolvedValueOnce(
        jsonResponse(
          {
            user: {
              id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
              email: "admin@example.com",
              is_admin: true,
              is_initial_admin: true,
            },
          },
          { status: 200 },
        ),
      );

    vi.stubGlobal("fetch", fetchMock);

    const router = createMemoryRouter(routes, {
      initialEntries: ["/login?redirect=%2Fprojects"],
    });
    const queryClient = new QueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );

    await userEvent.type(screen.getByLabelText(/email/i), "admin@example.com");
    await userEvent.type(screen.getByLabelText(/password/i), "secret-pass");
    await userEvent.click(screen.getByRole("button", { name: /sign in/i }));

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
    });
  });

  test("authenticated users visiting login are redirected away", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      jsonResponse(
        {
          user: {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            email: "admin@example.com",
            is_admin: true,
            is_initial_admin: true,
          },
        },
        { status: 200 },
      ),
    );

    vi.stubGlobal("fetch", fetchMock);

    const router = createMemoryRouter(routes, {
      initialEntries: ["/login?redirect=%2Fprojects%3Ffilter%3Dactive"],
    });
    const queryClient = new QueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/projects");
      expect(router.state.location.search).toBe("?filter=active");
    });
  });

  test("logout returns to login with redirect root", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse(
          {
            user: {
              id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
              email: "admin@example.com",
              is_admin: true,
              is_initial_admin: true,
            },
          },
          { status: 200 },
        ),
      )
      .mockResolvedValueOnce(new Response(null, { status: 204 }));

    vi.stubGlobal("fetch", fetchMock);

    const router = createMemoryRouter(routes, {
      initialEntries: ["/"],
    });
    const queryClient = new QueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );

    await userEvent.click(
      await screen.findByRole("button", { name: /sign out/i }),
    );

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/login");
      expect(router.state.location.search).toBe("?redirect=%2F");
    });
  });
});
