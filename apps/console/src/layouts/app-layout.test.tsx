import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { useTeam } from "@/features/teams/team-context";
import { BrandingProvider } from "@/features/branding/branding-context";
import { AppLayout } from "./app-layout";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));
vi.mock("@/features/teams/team-switcher", () => ({
  TeamSwitcher: () => <div>Team switcher</div>,
}));
vi.mock("@/hooks/use-mobile", () => ({ useIsMobile: () => false }));

function renderLayout(
  platformRole: "admin" | "user",
  teamRole: "owner" | "admin" | "member" | "viewer" = "owner",
) {
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
    activeRole: teamRole,
    error: null,
    refreshTeams: vi.fn(),
  } as ReturnType<typeof useTeam>);

  render(
    <BrandingProvider branding={{ siteName: "Acme Deploy", version: "0.1.0" }}>
      <MemoryRouter>
        <AppLayout />
      </MemoryRouter>
    </BrandingProvider>,
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

  it.each(["member", "viewer"] as const)("hides team Audit from %s", (role) => {
    renderLayout("user", role);
    expect(screen.queryByRole("link", { name: "Audit" })).not.toBeInTheDocument();
  });

  it.each(["owner", "admin"] as const)("shows team Audit to %s", (role) => {
    renderLayout("user", role);
    expect(screen.getByRole("link", { name: "Audit" })).toBeInTheDocument();
  });

  it("uses the configured site name in the sidebar", () => {
    renderLayout("user");
    expect(screen.getByRole("link", { name: "Acme Deploy Console" })).toBeInTheDocument();
    expect(screen.getByText("Acme Deploy")).toBeInTheDocument();
  });
});
