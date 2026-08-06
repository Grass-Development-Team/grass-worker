import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { adminApi, type AdminSettings } from "../admin.api";
import { AuthenticationPanel } from "./authentication-panel";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      getSettings: vi.fn(),
      updateSettings: vi.fn(),
      listIdentityProviders: vi.fn(),
    },
  };
});

const settings = {
  mail: { mode: "smtp" },
  authentication: {
    password_policy: {
      min_length: 8,
      max_length: 1024,
      require_lowercase: false,
      require_uppercase: false,
      require_number: false,
      require_symbol: false,
      history_count: 0,
    },
    registration_email_verification: false,
    mfa_policy: {
      allowed_factors: ["totp"],
      enforcement: "none",
      minimum_factors: 0,
      required_factors: [],
    },
  },
} as AdminSettings;

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(adminApi.getSettings).mockResolvedValue(settings);
  vi.mocked(adminApi.updateSettings).mockResolvedValue(settings);
  vi.mocked(adminApi.listIdentityProviders).mockResolvedValue({ providers: [] });
});

it("adds each MFA method through the table and saves its requirement", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const user = userEvent.setup();
  render(
    <QueryClientProvider client={client}>
      <AuthenticationPanel />
    </QueryClientProvider>,
  );

  expect(await screen.findByText("Authenticator app")).toBeInTheDocument();
  expect(screen.queryByText("Selected users")).not.toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Add method" }));
  await user.click(screen.getByRole("button", { name: /Email code/ }));

  const emailRow = screen.getByText("Email code").closest("tr");
  expect(emailRow).not.toBeNull();
  await user.click(within(emailRow!).getByRole("checkbox"));
  const minimum = screen.getByLabelText("Minimum enrolled methods");
  await user.clear(minimum);
  await user.type(minimum, "1");
  await user.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() =>
    expect(adminApi.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        mfa_policy: expect.objectContaining({
          allowed_factors: ["totp", "email"],
          minimum_factors: 1,
          required_factors: ["email"],
        }),
      }),
    ),
  );
});

it("turns enforcement into an actionable one-method requirement", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const user = userEvent.setup();
  render(
    <QueryClientProvider client={client}>
      <AuthenticationPanel />
    </QueryClientProvider>,
  );

  await screen.findByText("Authenticator app");
  await user.click(screen.getByLabelText("Enforcement scope"));
  await user.click(screen.getByRole("option", { name: "All users" }));
  await user.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() =>
    expect(adminApi.updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        mfa_policy: expect.objectContaining({
          enforcement: "all_users",
          minimum_factors: 1,
        }),
      }),
    ),
  );
});
