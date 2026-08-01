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
vi.mock("@/layouts/auth-layout", () => ({ AuthLayout: () => <Outlet /> }));
vi.mock("@/features/auth/login-route", () => ({
  LoginRoute: () => <div>Login page</div>,
}));
vi.mock("@/features/dashboard/dashboard-route", () => ({
  DashboardRoute: () => <div>Overview page</div>,
}));
vi.mock("@/features/admin/admin-route", () => ({
  AdminRoute: () => <div>Administration page</div>,
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
    logout: vi.fn(),
  } as ReturnType<typeof useAuth>);
}

function setGuest() {
  vi.mocked(useAuth).mockReturnValue({
    user: null,
    isLoading: false,
    login: vi.fn(),
    register: vi.fn(),
    logout: vi.fn(),
  } as ReturnType<typeof useAuth>);
}

describe("Administration routing", () => {
  beforeEach(() => vi.clearAllMocks());

  it("redirects a regular platform user away from /admin", () => {
    setUser("user");

    render(
      <MemoryRouter initialEntries={["/admin"]}>
        <Router />
      </MemoryRouter>,
    );

    expect(screen.getByText("Overview page")).toBeInTheDocument();
    expect(screen.queryByText("Administration page")).not.toBeInTheDocument();
  });

  it("allows a platform administrator to open /admin", () => {
    setUser("admin");

    render(
      <MemoryRouter initialEntries={["/admin"]}>
        <Router />
      </MemoryRouter>,
    );

    expect(screen.getByText("Administration page")).toBeInTheDocument();
  });
});

it("allows unauthenticated visitors to inspect an invitation link", () => {
  setGuest();

  render(
    <MemoryRouter initialEntries={["/invitations/accept?token=secret"]}>
      <Router />
    </MemoryRouter>,
  );

  expect(screen.getByText("Invitation page")).toBeInTheDocument();
});

it("allows authenticated users to open notifications", () => {
  setUser("user");

  render(
    <MemoryRouter initialEntries={["/notifications"]}>
      <Router />
    </MemoryRouter>,
  );

  expect(screen.getByText("Notifications page")).toBeInTheDocument();
});

it("redirects an unauthenticated visitor away from a protected route", () => {
  setGuest();

  render(
    <MemoryRouter initialEntries={["/notifications?filter=unread"]}>
      <Router />
    </MemoryRouter>,
  );

  expect(screen.getByText("Login page")).toBeInTheDocument();
  expect(screen.queryByText("Notifications page")).not.toBeInTheDocument();
});
