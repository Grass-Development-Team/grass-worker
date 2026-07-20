import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { useTeam } from "@/features/teams/team-context";
import { DashboardRoute } from "./dashboard-route";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));

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

  render(
    <MemoryRouter>
      <DashboardRoute />
    </MemoryRouter>,
  );
}

describe("Dashboard administration shortcut", () => {
  beforeEach(() => vi.clearAllMocks());

  it("hides Administration from a regular platform user", () => {
    renderDashboard("user");
    expect(screen.queryByRole("link", { name: "Administration" })).not.toBeInTheDocument();
  });

  it("shows Administration to a platform administrator", () => {
    renderDashboard("admin");
    expect(screen.getByRole("link", { name: "Administration" })).toBeInTheDocument();
  });
});
