import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { showErrorToast } from "@/lib/toast";
import { ProjectCreateLayout } from "./project-create-layout";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("@/features/notifications/notification-bell", () => ({ NotificationBell: () => null }));
vi.mock("@/lib/toast", () => ({ showErrorToast: vi.fn() }));

const logout = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  logout.mockResolvedValue(undefined);
  vi.mocked(useAuth).mockReturnValue({
    user: {
      id: "admin",
      email: "admin@example.invalid",
      display_name: "Admin",
      avatar_url: null,
      platform_role: "admin",
      email_verified: true,
    },
    isLoading: false,
    login: vi.fn(),
    register: vi.fn(),
    completeMfa: vi.fn(),
    verifyEmail: vi.fn(),
    updateProfile: vi.fn(),
    uploadAvatar: vi.fn(),
    removeAvatar: vi.fn(),
    logout,
  });
});

function renderLayout() {
  render(
    <MemoryRouter initialEntries={["/new"]}>
      <Routes>
        <Route path="/new" element={<ProjectCreateLayout />} />
        <Route path="/login" element={<div>Login page</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

it("keeps the creation menu restricted for administrators and redirects after logout", async () => {
  renderLayout();
  await userEvent.click(screen.getByRole("button", { name: "Open account menu" }));
  expect(screen.queryByRole("menuitem", { name: "Administration" })).not.toBeInTheDocument();
  expect(screen.getByRole("menuitem", { name: "Personal settings" })).toHaveAttribute(
    "href",
    "/account/profile",
  );
  await userEvent.click(screen.getByRole("menuitem", { name: "Log out" }));
  expect(logout).toHaveBeenCalledOnce();
  expect(await screen.findByText("Login page")).toBeInTheDocument();
});

it("keeps the creation page open when logout fails and reports the error", async () => {
  const error = new Error("Sign out failed");
  logout.mockRejectedValue(error);
  renderLayout();
  await userEvent.click(screen.getByRole("button", { name: "Open account menu" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Log out" }));
  await waitFor(() => expect(showErrorToast).toHaveBeenCalledWith(error));
  expect(screen.getByRole("link", { name: "Back" })).toHaveAttribute("href", "/projects");
  expect(screen.queryByText("Login page")).not.toBeInTheDocument();
});
