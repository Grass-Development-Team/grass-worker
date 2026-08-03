import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
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
vi.mock("@/features/notifications/notification-bell", () => ({
  NotificationBell: () => (
    <a href="/notifications" aria-label="Notifications">
      Notifications
    </a>
  ),
}));
vi.mock("@/hooks/use-mobile", () => ({ useIsMobile: () => false }));

function renderLayout(
  platformRole: "admin" | "user",
  teamRole: "owner" | "admin" | "member" | "viewer" = "owner",
  path = "/",
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

  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <BrandingProvider branding={{ siteName: "Acme Deploy", version: "0.1.0" }}>
        <MemoryRouter initialEntries={[path]}>
          <AppLayout />
        </MemoryRouter>
      </BrandingProvider>
    </QueryClientProvider>,
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

  it("shows the notifications entry in the application header", () => {
    renderLayout("user");
    expect(screen.getByRole("link", { name: "Notifications" })).toHaveAttribute(
      "href",
      "/notifications",
    );
  });

  it("replaces the administration sidebar inside Settings", () => {
    renderLayout("admin", "owner", "/admin/settings/basic");

    expect(screen.getByRole("link", { name: "Administration" })).toHaveAttribute("href", "/admin");
    expect(screen.getByRole("link", { name: "Basic" })).toHaveAttribute(
      "href",
      "/admin/settings/basic",
    );
    expect(screen.getByRole("link", { name: "Announcements" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Reviews" })).not.toBeInTheDocument();
  });

  it("keeps the existing Administration return navigation", () => {
    renderLayout("admin", "owner", "/admin/reviews");

    expect(screen.getByRole("link", { name: "Console" })).toHaveAttribute("href", "/");
    expect(screen.getByRole("link", { name: "Reviews" })).toBeInTheDocument();
  });

  it("uses a dedicated personal settings sidebar", () => {
    renderLayout("user", "owner", "/account/profile");

    expect(screen.getByRole("link", { name: "Console" })).toHaveAttribute("href", "/");
    expect(screen.getByRole("link", { name: "Profile" })).toHaveAttribute(
      "href",
      "/account/profile",
    );
    expect(screen.queryByRole("link", { name: "Projects" })).not.toBeInTheDocument();
  });
});
