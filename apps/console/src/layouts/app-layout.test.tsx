import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { useTeam } from "@/features/teams/team-context";
import { AppLayout } from "./app-layout";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));
vi.mock("@/features/teams/team-switcher", () => ({
  TeamSwitcher: () => <div>Team switcher</div>,
}));
vi.mock("@/hooks/use-mobile", () => ({ useIsMobile: () => false }));

function renderLayout(platformRole: "admin" | "user") {
  vi.mocked(useAuth).mockReturnValue({
    user: {
      id: "user-1",
      email: "user@example.com",
      display_name: "User",
      platform_role: platformRole,
    },
    logout: vi.fn(),
  } as ReturnType<typeof useAuth>);
  vi.mocked(useTeam).mockReturnValue({
    activeTeam: { id: "team-1", slug: "team", name: "Team", kind: "team" },
    activeRole: "owner",
    error: null,
    refreshTeams: vi.fn(),
  } as ReturnType<typeof useTeam>);

  render(
    <MemoryRouter>
      <AppLayout />
    </MemoryRouter>,
  );
}

describe("App layout administration navigation", () => {
  beforeEach(() => vi.clearAllMocks());

  it("hides Administration from a regular platform user", () => {
    renderLayout("user");
    expect(screen.queryByRole("link", { name: "Administration" })).not.toBeInTheDocument();
  });

  it("shows Administration to a platform administrator", () => {
    renderLayout("admin");
    expect(screen.getByRole("link", { name: "Administration" })).toBeInTheDocument();
  });
});
