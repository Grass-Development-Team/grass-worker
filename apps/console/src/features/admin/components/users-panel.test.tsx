import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { adminApi, type AdminUser } from "../admin.api";
import { UsersPanel } from "./users-panel";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      listUsers: vi.fn(),
      listUserMfa: vi.fn(),
      updateUserMfaPolicy: vi.fn(),
    },
  };
});

const users: AdminUser[] = [
  {
    id: "user-1",
    email: "admin@example.com",
    display_name: "Admin",
    status: "active",
    platform_role: "admin",
    email_verified: true,
    last_login_at: null,
    created_at: "2026-08-04T00:00:00Z",
  },
  {
    id: "user-2",
    email: "user@example.com",
    display_name: "User",
    status: "active",
    platform_role: "user",
    email_verified: true,
    last_login_at: null,
    created_at: "2026-08-04T00:00:00Z",
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(useAuth).mockReturnValue({
    user: { id: "user-1", email: "admin@example.com", platform_role: "admin" },
  } as ReturnType<typeof useAuth>);
  vi.mocked(adminApi.listUsers).mockResolvedValue({ users });
  vi.mocked(adminApi.listUserMfa).mockResolvedValue({
    factors: [],
    policy: { inherit_platform: true, minimum_factors: 0, required_factors: [] },
    allowed_factors: ["totp", "email"],
    effective_requirements: { minimum_factors: 0, required_factors: [] },
  });
  vi.mocked(adminApi.updateUserMfaPolicy).mockResolvedValue({
    policy: { inherit_platform: false, minimum_factors: 1, required_factors: ["totp"] },
    effective_requirements: { minimum_factors: 1, required_factors: ["totp"] },
  });
});

it("selects visible users and saves a per-user MFA policy", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const user = userEvent.setup();
  render(
    <QueryClientProvider client={client}>
      <UsersPanel />
    </QueryClientProvider>,
  );

  await screen.findByText("admin@example.com");
  await user.click(screen.getByRole("checkbox", { name: "Select all visible users" }));
  expect(screen.getByText("2 users selected")).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Actions for user@example.com" }));
  await user.click(screen.getByRole("menuitem", { name: "Manage MFA" }));
  expect(await screen.findByText("Effective minimum: 0")).toBeInTheDocument();
  await user.click(screen.getByRole("checkbox", { name: "Use a custom policy for this user" }));
  const minimum = screen.getByLabelText("Minimum enrolled methods");
  await user.clear(minimum);
  await user.type(minimum, "1");
  await user.click(screen.getByRole("checkbox", { name: "Authenticator app" }));
  await user.click(screen.getByRole("button", { name: "Save policy" }));

  await waitFor(() =>
    expect(adminApi.updateUserMfaPolicy).toHaveBeenCalledWith("user-2", {
      inherit_platform: false,
      minimum_factors: 1,
      required_factors: ["totp"],
    }),
  );
});
