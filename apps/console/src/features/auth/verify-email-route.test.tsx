import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { authApi } from "./auth.api";
import { useAuth } from "./auth-context";
import { VerifyEmailRoute } from "./verify-email-route";

vi.mock("./auth-context", () => ({ useAuth: vi.fn() }));

afterEach(() => {
  vi.restoreAllMocks();
});

it("returns to a local invitation after email verification", async () => {
  const verifyEmail = vi.fn().mockResolvedValue(undefined);
  vi.mocked(useAuth).mockReturnValue({ verifyEmail } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter
      initialEntries={[
        "/verify-email?token=verification-token&return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token",
      ]}
    >
      <Routes>
        <Route path="/verify-email" element={<VerifyEmailRoute />} />
        <Route path="/invitations/accept" element={<div>accept invitation</div>} />
      </Routes>
    </MemoryRouter>,
  );

  await waitFor(() => expect(screen.getByText("accept invitation")).toBeInTheDocument());
  expect(verifyEmail).toHaveBeenCalledWith("verification-token");
});

it("preserves a local return destination when resending verification", async () => {
  const user = userEvent.setup();
  vi.mocked(useAuth).mockReturnValue({ verifyEmail: vi.fn() } as unknown as ReturnType<
    typeof useAuth
  >);
  const resend = vi.spyOn(authApi, "resendVerification").mockResolvedValue({ accepted: true });

  render(
    <MemoryRouter
      initialEntries={[
        "/verify-email?email=invitee%40example.com&return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token",
      ]}
    >
      <VerifyEmailRoute />
    </MemoryRouter>,
  );

  await user.click(screen.getByRole("button", { name: "Resend verification" }));

  expect(resend).toHaveBeenCalledWith(
    "invitee@example.com",
    "/invitations/accept?token=invite-token",
  );
});
