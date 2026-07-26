import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { useTeam } from "@/features/teams/team-context";
import { DashboardRoute } from "./dashboard-route";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function renderDashboard(platformRole: "admin" | "user") {
  vi.mocked(useAuth).mockReturnValue({
    user: {
      id: "user-1",
      email: "user@example.com",
      display_name: "User",
      platform_role: platformRole,
    },
  } as ReturnType<typeof useAuth>);
  vi.mocked(useTeam).mockReturnValue({
    activeTeam: { id: "team-1", slug: "team", name: "Team", kind: "team" },
    activeRole: "owner",
  } as ReturnType<typeof useTeam>);

  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <DashboardRoute />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("Dashboard administration shortcut", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(globalThis, "fetch").mockImplementation(async () => jsonResponse({ projects: [] }));
  });
  afterEach(() => vi.restoreAllMocks());

  it("hides Administration from a regular platform user", () => {
    renderDashboard("user");
    expect(screen.queryByRole("link", { name: "Administration" })).not.toBeInTheDocument();
  });

  it("shows Administration to a platform administrator", () => {
    renderDashboard("admin");
    expect(screen.getByRole("link", { name: "Administration" })).toBeInTheDocument();
  });
});
