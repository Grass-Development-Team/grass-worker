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
    user: {
      id: "user-1",
      email: "user@example.com",
      display_name: "Old Name",
      platform_role: "user",
    },
    updateProfile,
  } as ReturnType<typeof useAuth>);
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
