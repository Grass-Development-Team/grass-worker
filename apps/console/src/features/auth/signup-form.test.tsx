import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { LoginForm } from "./login-form";
import { SignupForm } from "./signup-form";
import { useAuth } from "./auth-context";

vi.mock("./auth-context", () => ({ useAuth: vi.fn() }));

afterEach(() => {
  vi.clearAllMocks();
});

it("keeps the invitation token in the login sign-up link", () => {
  vi.mocked(useAuth).mockReturnValue({ login: vi.fn() } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter initialEntries={["/login?invitation_token=invite-token"]}>
      <LoginForm />
    </MemoryRouter>,
  );

  expect(screen.getByRole("link", { name: "Sign up" })).toHaveAttribute(
    "href",
    "/signup?invitation_token=invite-token",
  );
});

it("keeps an invitation token from a protected-route redirect in the sign-up link", () => {
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
    "/signup?invitation_token=invite-token",
  );
});

it("continues an invitation after an existing user logs in", async () => {
  const user = userEvent.setup();
  const login = vi.fn().mockResolvedValue(undefined);
  vi.mocked(useAuth).mockReturnValue({ login } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter initialEntries={["/login?invitation_token=invite-token"]}>
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
  expect(login).toHaveBeenCalledWith("leo@example.com", "password123");
});

it("registers, enters the application, and keeps the token in the login link", async () => {
  const user = userEvent.setup();
  const register = vi.fn().mockResolvedValue(undefined);
  vi.mocked(useAuth).mockReturnValue({ register } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter initialEntries={["/signup?invitation_token=invite-token"]}>
      <Routes>
        <Route path="/signup" element={<SignupForm />} />
        <Route path="/" element={<div>application</div>} />
      </Routes>
    </MemoryRouter>,
  );

  await user.type(screen.getByLabelText("Email"), " leo@example.com ");
  await user.type(screen.getByLabelText("Display name"), " Leo ");
  await user.type(screen.getByLabelText("Password", { selector: "#password" }), "password123");
  await user.type(screen.getByLabelText("Confirm password"), "password123");
  expect(screen.getByRole("link", { name: "Log in" })).toHaveAttribute(
    "href",
    "/login?invitation_token=invite-token",
  );

  await user.click(screen.getByRole("button", { name: "Create account" }));

  expect(register).toHaveBeenCalledWith({
    email: "leo@example.com",
    display_name: "Leo",
    password: "password123",
    invitation_token: "invite-token",
  });
  await waitFor(() => expect(screen.getByText("application")).toBeInTheDocument());
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
