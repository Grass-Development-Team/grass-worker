import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { authApi } from "@/features/auth/auth.api";
import { SecurityRoute } from "./security-route";

vi.mock("@/features/auth/auth.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/auth/auth.api")>();
  return {
    ...actual,
    authApi: {
      ...actual.authApi,
      security: vi.fn(),
    },
  };
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(authApi.security).mockResolvedValue({
    email_verified: true,
    factors: [
      {
        id: "factor-1",
        kind: "totp",
        verified: true,
        created_at: "not-a-date",
        last_used_at: null,
      },
    ],
    allowed_factors: ["totp", "email"],
    mfa_required: true,
    mfa_requirements: { minimum_factors: 1, required_factors: ["totp"] },
    mfa_policy: { inherit_platform: true, minimum_factors: 0, required_factors: [] },
    password_policy: {
      min_length: 8,
      max_length: 1024,
      require_lowercase: false,
      require_uppercase: false,
      require_number: false,
      require_symbol: false,
      history_count: 0,
    },
    mail_available: true,
  });
});

it("renders MFA methods as a policy table and never displays Invalid Date", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <SecurityRoute />
    </QueryClientProvider>,
  );

  expect(await screen.findByText("Authenticator app")).toBeInTheDocument();
  expect(screen.getByText("Email code")).toBeInTheDocument();
  expect(screen.getByText("Unknown date")).toBeInTheDocument();
  expect(screen.queryByText("Invalid Date")).not.toBeInTheDocument();
  expect(screen.getByText("Required")).toBeInTheDocument();
  expect(screen.getAllByText("Not configured")).toHaveLength(2);
});
