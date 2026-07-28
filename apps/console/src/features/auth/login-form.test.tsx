import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { LoginForm, previewAuthorizationContinuation } from "./login-form";
import { useAuth } from "./auth-context";

vi.mock("./auth-context", () => ({ useAuth: vi.fn() }));

afterEach(() => {
  vi.clearAllMocks();
});

it("accepts only the Preview authorization API continuation", () => {
  expect(
    previewAuthorizationContinuation(
      "?continue=%2Fapi%2Fv1%2Fpreview%2Fauthorize%3Fstate%3Dopaque",
      "https://cxcs.page",
    ),
  ).toBe("/api/v1/preview/authorize?state=opaque");

  for (const search of [
    "?continue=https%3A%2F%2Fevil.test%2F",
    "?continue=%2F%2Fevil.test%2F",
    "?continue=%2Fapi%2Fv1%2Fpreview%2Fauthorize",
    "?continue=%2Fapi%2Fv1%2Fpreview%2Fauthorize%3Fstate%3D",
    "?continue=%2Fapi%2Fv1%2Fprojects",
  ]) {
    expect(previewAuthorizationContinuation(search, "https://cxcs.page")).toBeNull();
  }
});

it("uses a full-page navigation after Preview login", async () => {
  const user = userEvent.setup();
  const login = vi.fn().mockResolvedValue(undefined);
  const documentNavigate = vi.fn();
  vi.mocked(useAuth).mockReturnValue({ login } as unknown as ReturnType<typeof useAuth>);

  render(
    <MemoryRouter
      initialEntries={["/login?continue=%2Fapi%2Fv1%2Fpreview%2Fauthorize%3Fstate%3Dopaque"]}
    >
      <LoginForm documentNavigate={documentNavigate} />
    </MemoryRouter>,
  );

  await user.type(screen.getByLabelText("Email"), "leo@example.com");
  await user.type(screen.getByLabelText("Password"), "password123");
  await user.click(screen.getByRole("button", { name: "Login" }));

  await waitFor(() =>
    expect(documentNavigate).toHaveBeenCalledWith("/api/v1/preview/authorize?state=opaque"),
  );
  expect(login).toHaveBeenCalledWith("leo@example.com", "password123");
});
