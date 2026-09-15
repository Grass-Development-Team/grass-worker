import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { ProfileRoute } from "./profile-route";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));

const updateProfile = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  updateProfile.mockResolvedValue(undefined);
  vi.mocked(useAuth).mockReturnValue({
    isLoading: false,
    login: vi.fn(),
    register: vi.fn(),
    completeMfa: vi.fn(),
    verifyEmail: vi.fn(),
    uploadAvatar: vi.fn(),
    removeAvatar: vi.fn(),
    logout: vi.fn(),
    user: {
      avatar_url: null,
      email_verified: true,
      id: "user-1",
      email: "user@example.com",
      display_name: "Old Name",
      platform_role: "user",
    },
    updateProfile,
  });
});

it("updates the display name while keeping the email read-only", async () => {
  const user = userEvent.setup();
  render(<ProfileRoute />);

  const name = screen.getByLabelText("Display name");
  expect(screen.getByLabelText("Email")).toHaveAttribute("readonly");
  await user.clear(name);
  await user.type(name, "New Name");
  await user.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => expect(updateProfile).toHaveBeenCalledWith("New Name"));
  expect(screen.getByText("Saved.")).toBeInTheDocument();
});
