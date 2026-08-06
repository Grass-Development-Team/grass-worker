import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { App } from "./App";

vi.mock("@/features/auth/auth-context", () => ({
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
  useAuth: () => ({ user: null, isLoading: false, login: vi.fn() }),
}));

afterEach(() => {
  document.title = "Console";
  vi.restoreAllMocks();
});

it("uses the public site configuration on the login page and document title", async () => {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL) => {
    if (String(input) === "/health") {
      return Response.json({ status: "ok", service: "Grass Worker API", version: "0.1.0" });
    }
    if (String(input) === "/api/v1/site-config") {
      return Response.json({
        code: 200,
        message: "OK",
        data: { site_name: "Acme Deploy", logo_url: "/brand.svg", version: "0.1.0" },
      });
    }
    throw new Error(`unexpected request: ${input}`);
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={["/login"]}>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );

  expect(
    await screen.findByRole("heading", { name: "Welcome to Acme Deploy" }),
  ).toBeInTheDocument();
  expect(document.title).toBe("Acme Deploy");
  const logo = document.querySelector('img[src="/brand.svg"]');
  expect(logo).toBeInTheDocument();
  expect(logo?.parentElement).not.toHaveClass("bg-primary");
  expect(document.querySelector('link[data-branding-favicon="true"]')).toHaveAttribute(
    "href",
    "/brand.svg",
  );
});
