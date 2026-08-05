import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

import { LoginForm } from "./login-form";
import { SignupForm } from "./signup-form";
import { useAuth } from "./auth-context";
import { BrandingProvider } from "@/features/branding/branding-context";
import { useAuthConfiguration } from "./provider-buttons";

vi.mock("./auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("./provider-buttons", () => ({
  useAuthConfiguration: vi.fn(),
  ProviderButtons: ({
    registrationCode,
    returnTo,
  }: {
    registrationCode?: string;
    returnTo?: string;
  }) => (
    <>
      {registrationCode ? (
        <output data-testid="provider-registration-code">{registrationCode}</output>
      ) : null}
      {returnTo ? <output data-testid="provider-return-to">{returnTo}</output> : null}
    </>
  ),
}));

beforeEach(() => {
  vi.mocked(useAuthConfiguration).mockReturnValue({
    providers: [],
    password_recovery_available: true,
    registration_email_verification: false,
    signup_policy: "open",
    password_policy: {
      min_length: 8,
      max_length: 1024,
      require_lowercase: false,
      require_uppercase: false,
      require_number: false,
      require_symbol: false,
      history_count: 0,
    },
  });
});

afterEach(() => {
  vi.clearAllMocks();
});

it("keeps a local return destination in the login sign-up link", () => {
  vi.mocked(useAuth).mockReturnValue({ login: vi.fn() } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter
      initialEntries={["/login?return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token"]}
    >
      <LoginForm />
    </MemoryRouter>,
  );

  expect(screen.getByRole("link", { name: "Sign up" })).toHaveAttribute(
    "href",
    "/signup?return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token",
  );
});

it("keeps a local protected-route redirect in the sign-up link", () => {
  vi.mocked(useAuth).mockReturnValue({ login: vi.fn() } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter
      initialEntries={[
        {
          pathname: "/login",
          state: { from: "/invitations/accept?token=invite-token" },
        },
      ]}
    >
      <LoginForm />
    </MemoryRouter>,
  );

  expect(screen.getByRole("link", { name: "Sign up" })).toHaveAttribute(
    "href",
    "/signup?return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token",
  );
});

it("continues an invitation after an existing user logs in", async () => {
  const user = userEvent.setup();
  const login = vi.fn().mockResolvedValue({
    user: {
      id: "user-1",
      email: "leo@example.com",
      display_name: "Leo",
      platform_role: "user",
      email_verified: true,
    },
    csrf_token: "csrf-token",
  });
  vi.mocked(useAuth).mockReturnValue({ login } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter
      initialEntries={["/login?return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token"]}
    >
      <Routes>
        <Route path="/login" element={<LoginForm />} />
        <Route path="/invitations/accept" element={<div>accept invitation</div>} />
      </Routes>
    </MemoryRouter>,
  );

  await user.type(screen.getByLabelText("Email"), "leo@example.com");
  await user.type(screen.getByLabelText("Password"), "password123");
  await user.click(screen.getByRole("button", { name: "Login" }));

  await waitFor(() => expect(screen.getByText("accept invitation")).toBeInTheDocument());
  expect(login).toHaveBeenCalledWith(
    "leo@example.com",
    "password123",
    "/invitations/accept?token=invite-token",
  );
});

it("registers and returns to the invitation without sending its token as a credential", async () => {
  const user = userEvent.setup();
  const register = vi.fn().mockResolvedValue({
    user: {
      id: "user-1",
      email: "leo@example.com",
      display_name: "Leo",
      platform_role: "user",
      email_verified: true,
    },
    csrf_token: "csrf-token",
  });
  vi.mocked(useAuth).mockReturnValue({ register } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter
      initialEntries={["/signup?return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token"]}
    >
      <Routes>
        <Route path="/signup" element={<SignupForm />} />
        <Route path="/invitations/accept" element={<div>accept invitation</div>} />
      </Routes>
    </MemoryRouter>,
  );

  await user.type(screen.getByLabelText("Email"), " leo@example.com ");
  await user.type(screen.getByLabelText("Display name"), " Leo ");
  await user.type(screen.getByLabelText("Password", { selector: "#password" }), "password123");
  await user.type(screen.getByLabelText("Confirm password"), "password123");
  expect(screen.getByRole("link", { name: "Log in" })).toHaveAttribute(
    "href",
    "/login?return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token",
  );
  expect(screen.getByTestId("provider-return-to")).toHaveTextContent(
    "/invitations/accept?token=invite-token",
  );

  await user.click(screen.getByRole("button", { name: "Create account" }));

  expect(register).toHaveBeenCalledWith({
    email: "leo@example.com",
    display_name: "Leo",
    password: "password123",
    return_to: "/invitations/accept?token=invite-token",
  });
  await waitFor(() => expect(screen.getByText("accept invitation")).toBeInTheDocument());
});

it("continues registration on the email verification page", async () => {
  const user = userEvent.setup();
  const register = vi.fn().mockResolvedValue({
    verification_required: true,
    email: "leo@example.com",
  });
  vi.mocked(useAuth).mockReturnValue({ register } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter
      initialEntries={["/signup?return_to=%2Finvitations%2Faccept%3Ftoken%3Dinvite-token"]}
    >
      <Routes>
        <Route path="/signup" element={<SignupForm />} />
        <Route path="/verify-email" element={<div>Verify email page</div>} />
      </Routes>
    </MemoryRouter>,
  );

  await user.type(screen.getByLabelText("Email"), "leo@example.com");
  await user.type(screen.getByLabelText("Display name"), "Leo");
  await user.type(screen.getByLabelText("Password", { selector: "#password" }), "password123");
  await user.type(screen.getByLabelText("Confirm password"), "password123");
  await user.click(screen.getByRole("button", { name: "Create account" }));

  expect(await screen.findByText("Verify email page")).toBeInTheDocument();
  expect(register).toHaveBeenCalledWith(
    expect.objectContaining({ return_to: "/invitations/accept?token=invite-token" }),
  );
});

it("uses an optional registration code for invite-only password and provider signup", async () => {
  const user = userEvent.setup();
  const register = vi.fn().mockResolvedValue({
    user: {
      id: "user-1",
      email: "leo@example.com",
      display_name: "Leo",
      platform_role: "user",
      email_verified: true,
    },
    csrf_token: "csrf-token",
  });
  vi.mocked(useAuth).mockReturnValue({ register } as unknown as ReturnType<typeof useAuth>);
  vi.mocked(useAuthConfiguration).mockReturnValue({
    ...vi.mocked(useAuthConfiguration)()!,
    signup_policy: "invite_only",
    providers: [{ slug: "github", name: "GitHub", kind: "github" }],
  });

  render(
    <MemoryRouter initialEntries={["/signup"]}>
      <Routes>
        <Route path="/signup" element={<SignupForm />} />
        <Route path="/" element={<div>application</div>} />
      </Routes>
    </MemoryRouter>,
  );

  await user.type(screen.getByLabelText("Registration code (optional)"), "registration-code");
  expect(screen.getByTestId("provider-registration-code")).toHaveTextContent("registration-code");
  await user.type(screen.getByLabelText("Email"), "leo@example.com");
  await user.type(screen.getByLabelText("Display name"), "Leo");
  await user.type(screen.getByLabelText("Password", { selector: "#password" }), "password123");
  await user.type(screen.getByLabelText("Confirm password"), "password123");
  await user.click(screen.getByRole("button", { name: "Create account" }));

  expect(register).toHaveBeenCalledWith({
    email: "leo@example.com",
    display_name: "Leo",
    password: "password123",
    registration_code: "registration-code",
  });
});

it("hides signup controls when registration is closed", () => {
  vi.mocked(useAuth).mockReturnValue({ register: vi.fn() } as unknown as ReturnType<
    typeof useAuth
  >);
  vi.mocked(useAuthConfiguration).mockReturnValue({
    ...vi.mocked(useAuthConfiguration)()!,
    signup_policy: "closed",
  });

  render(
    <MemoryRouter initialEntries={["/signup"]}>
      <SignupForm />
    </MemoryRouter>,
  );

  expect(screen.getByText("Registration is closed")).toBeInTheDocument();
  expect(screen.queryByLabelText("Email")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Create account" })).not.toBeInTheDocument();
});

it("does not request a registration code for open signup", () => {
  vi.mocked(useAuth).mockReturnValue({ register: vi.fn() } as unknown as ReturnType<
    typeof useAuth
  >);

  render(
    <MemoryRouter initialEntries={["/signup"]}>
      <SignupForm />
    </MemoryRouter>,
  );

  expect(screen.queryByLabelText("Registration code (optional)")).not.toBeInTheDocument();
});

it("rejects mismatched passwords without registering", async () => {
  const user = userEvent.setup();
  const register = vi.fn();
  vi.mocked(useAuth).mockReturnValue({ register } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter initialEntries={["/signup"]}>
      <SignupForm />
    </MemoryRouter>,
  );

  await user.type(screen.getByLabelText("Email"), "leo@example.com");
  await user.type(screen.getByLabelText("Display name"), "Leo");
  await user.type(screen.getByLabelText("Password", { selector: "#password" }), "password123");
  await user.type(screen.getByLabelText("Confirm password"), "different-password");
  await user.click(screen.getByRole("button", { name: "Create account" }));

  expect(screen.getByText("Passwords do not match.")).toBeInTheDocument();
  expect(register).not.toHaveBeenCalled();
});

it("uses the configured site name on the registration page", () => {
  vi.mocked(useAuth).mockReturnValue({ register: vi.fn() } as unknown as ReturnType<
    typeof useAuth
  >);

  render(
    <BrandingProvider branding={{ siteName: "Acme Deploy", version: "0.1.0" }}>
      <MemoryRouter initialEntries={["/signup"]}>
        <SignupForm />
      </MemoryRouter>
    </BrandingProvider>,
  );

  expect(
    screen.getByRole("heading", { name: "Create your Acme Deploy account" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("Create your Grass Worker account")).not.toBeInTheDocument();
});
