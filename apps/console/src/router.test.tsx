import { render, screen } from "@testing-library/react";
import { MemoryRouter, Outlet } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { Router } from "./router";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("@/features/teams/team-context", () => ({
  TeamProvider: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@/layouts/app-layout", () => ({ AppLayout: () => <Outlet /> }));
vi.mock("@/layouts/project-create-layout", () => ({ ProjectCreateLayout: () => <Outlet /> }));
vi.mock("@/layouts/auth-layout", () => ({ AuthLayout: () => <Outlet /> }));
vi.mock("@/features/auth/login-route", () => ({
  LoginRoute: () => <div>Login page</div>,
}));
vi.mock("@/features/dashboard/dashboard-route", () => ({
  DashboardRoute: () => <div>Overview page</div>,
}));
vi.mock("@/features/admin/admin-route", () => ({
  AdminRoute: () => (
    <>
      <div>Administration page</div>
      <Outlet />
    </>
  ),
}));
vi.mock("@/features/admin/components/settings-layout", () => ({
  SettingsLayout: () => <Outlet />,
}));
vi.mock("@/features/admin/components/settings-panel", () => ({
  SettingsPanel: ({ section }: { section?: string }) => <div>Settings {section}</div>,
}));
vi.mock("@/features/admin/components/announcements-panel", () => ({
  AnnouncementsPanel: () => <div>Announcements settings</div>,
}));
vi.mock("@/features/admin/components/cleanup-panel", () => ({
  CleanupPanel: () => <div>Cleanup settings</div>,
}));
vi.mock("@/features/account/profile-route", () => ({
  ProfileRoute: () => <div>Profile settings</div>,
}));
vi.mock("@/features/teams/team-settings-guard", () => ({
  TeamSettingsGuard: () => <Outlet />,
}));
vi.mock("@/features/teams/accept-invitation-route", () => ({
  AcceptInvitationRoute: () => <div>Invitation page</div>,
}));
vi.mock("@/features/notifications/notifications-route", () => ({
  NotificationsRoute: () => <div>Notifications page</div>,
}));
vi.mock("@/features/projects/project-create-route", () => ({
  ProjectCreateRoute: () => <div>Create project page</div>,
}));

function setUser(platformRole: "admin" | "user") {
  vi.mocked(useAuth).mockReturnValue({
    user: {
      id: "user-1",
      email: "user@example.com",
      display_name: "User",
      platform_role: platformRole,
    },
    isLoading: false,
    login: vi.fn(),
    register: vi.fn(),
    updateProfile: vi.fn(),
    logout: vi.fn(),
  } as ReturnType<typeof useAuth>);
}

function setGuest() {
  vi.mocked(useAuth).mockReturnValue({
    user: null,
    isLoading: false,
    login: vi.fn(),
    register: vi.fn(),
    updateProfile: vi.fn(),
    logout: vi.fn(),
  } as ReturnType<typeof useAuth>);
}

describe("Administration routing", () => {
  beforeEach(() => vi.clearAllMocks());

  it("redirects a regular platform user away from /admin", async () => {
    setUser("user");

    render(
      <MemoryRouter initialEntries={["/admin"]}>
        <Router />
      </MemoryRouter>,
    );

    expect(await screen.findByText("Overview page")).toBeInTheDocument();
    expect(screen.queryByText("Administration page")).not.toBeInTheDocument();
  });

  it("allows a platform administrator to open /admin", async () => {
    setUser("admin");

    render(
      <MemoryRouter initialEntries={["/admin"]}>
        <Router />
      </MemoryRouter>,
    );

    expect(await screen.findByText("Administration page")).toBeInTheDocument();
  });

  it("redirects the settings index to the basic settings page", async () => {
    setUser("admin");

    render(
      <MemoryRouter initialEntries={["/admin/settings"]}>
        <Router />
      </MemoryRouter>,
    );

    expect(await screen.findByText("Settings basic")).toBeInTheDocument();
  });

  it("opens announcement settings as a nested administration page", async () => {
    setUser("admin");

    render(
      <MemoryRouter initialEntries={["/admin/settings/announcements"]}>
        <Router />
      </MemoryRouter>,
    );

    expect(await screen.findByText("Announcements settings")).toBeInTheDocument();
  });

  it("opens cleanup controls as a nested administration page", async () => {
    setUser("admin");

    render(
      <MemoryRouter initialEntries={["/admin/cleanup"]}>
        <Router />
      </MemoryRouter>,
    );

    expect(await screen.findByText("Cleanup settings")).toBeInTheDocument();
  });
});

it("allows unauthenticated visitors to inspect an invitation link", async () => {
  setGuest();

  render(
    <MemoryRouter initialEntries={["/invitations/accept?token=secret"]}>
      <Router />
    </MemoryRouter>,
  );

  expect(await screen.findByText("Invitation page")).toBeInTheDocument();
});

it("allows authenticated users to open notifications", async () => {
  setUser("user");

  render(
    <MemoryRouter initialEntries={["/notifications"]}>
      <Router />
    </MemoryRouter>,
  );

  expect(await screen.findByText("Notifications page")).toBeInTheDocument();
});

it("allows authenticated users to open personal settings", async () => {
  setUser("user");

  render(
    <MemoryRouter initialEntries={["/account/profile"]}>
      <Router />
    </MemoryRouter>,
  );

  expect(await screen.findByText("Profile settings")).toBeInTheDocument();
});

it("allows authenticated users to open the full-screen project creation route", async () => {
  setUser("user");

  render(
    <MemoryRouter initialEntries={["/projects/new"]}>
      <Router />
    </MemoryRouter>,
  );

  expect(await screen.findByText("Create project page")).toBeInTheDocument();
});

it("redirects an unauthenticated visitor away from a protected route", async () => {
  setGuest();

  render(
    <MemoryRouter initialEntries={["/notifications?filter=unread"]}>
      <Router />
    </MemoryRouter>,
  );

  expect(await screen.findByText("Login page")).toBeInTheDocument();
  expect(screen.queryByText("Notifications page")).not.toBeInTheDocument();
});
