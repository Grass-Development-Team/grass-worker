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
      mfaTotpStart: vi.fn(),
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
  const completeMfa = vi.fn().mockResolvedValue({
    user: {
      id: "user-1",
      email: "user@example.com",
      display_name: null,
      platform_role: "user",
      email_verified: true,
    },
    csrf_token: "csrf-token",
  });
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

it("continues enrollment until every required MFA method is configured", async () => {
  window.history.replaceState(null, "", "/mfa#challenge=challenge-token");
  vi.mocked(authApi.mfaChallenge).mockResolvedValue({
    mfa_required: false,
    mfa_enrollment_required: true,
    challenge_token: "challenge-token",
    factors: [],
    allowed_factors: ["totp", "email"],
    return_to: "/projects",
  });
  vi.mocked(authApi.mfaTotpStart).mockResolvedValue({
    factor: {
      id: "factor-totp",
      kind: "totp",
      verified: false,
      created_at: "2026-08-04T00:00:00Z",
      last_used_at: null,
    },
    secret: "ABCDEFGHIJKLMNOP",
    otpauth_uri: "otpauth://totp/Grass:user@example.com?secret=ABCDEFGHIJKLMNOP",
  });
  const completeMfa = vi.fn().mockResolvedValue({
    mfa_required: false,
    mfa_enrollment_required: true,
    challenge_token: "challenge-token",
    factors: [
      {
        id: "factor-totp",
        kind: "totp",
        verified: true,
        created_at: "2026-08-04T00:00:00Z",
        last_used_at: "2026-08-04T00:01:00Z",
      },
    ],
    allowed_factors: ["totp", "email"],
    return_to: "/projects",
  });
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

  expect(await screen.findByRole("button", { name: "Email code" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Authenticator app" })).not.toBeInTheDocument();
  expect(screen.queryByText("Projects page")).not.toBeInTheDocument();
});
