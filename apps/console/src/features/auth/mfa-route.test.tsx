import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { authApi } from "./auth.api";
import { useAuth } from "./auth-context";
import { MfaRoute } from "./mfa-route";

vi.mock("./auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("./auth.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./auth.api")>();
  return {
    ...actual,
    authApi: {
      ...actual.authApi,
      mfaChallenge: vi.fn(),
    },
  };
});

afterEach(() => {
  vi.clearAllMocks();
  window.history.replaceState(null, "", "/");
});

it("verifies an enrolled TOTP factor and returns to the challenge destination", async () => {
  window.history.replaceState(null, "", "/mfa#challenge=challenge-token");
  vi.mocked(authApi.mfaChallenge).mockResolvedValue({
    mfa_required: true,
    mfa_enrollment_required: false,
    challenge_token: "challenge-token",
    factors: [
      {
        id: "factor-1",
        kind: "totp",
        verified: true,
        created_at: "2026-08-04T00:00:00Z",
        last_used_at: null,
      },
    ],
    allowed_factors: ["totp"],
    return_to: "/projects",
  });
  const completeMfa = vi.fn().mockResolvedValue(undefined);
  vi.mocked(useAuth).mockReturnValue({ completeMfa } as unknown as ReturnType<typeof useAuth>);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const user = userEvent.setup();

  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={["/mfa"]}>
        <Routes>
          <Route path="/mfa" element={<MfaRoute />} />
          <Route path="/projects" element={<div>Projects page</div>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "Authenticator app" }));
  await user.type(screen.getByLabelText("Verification code"), "123456");
  await user.click(screen.getByRole("button", { name: "Continue" }));

  expect(await screen.findByText("Projects page")).toBeInTheDocument();
  expect(completeMfa).toHaveBeenCalledWith("challenge-token", "factor-1", "123456");
});
