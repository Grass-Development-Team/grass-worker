import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { AcceptInvitationRoute } from "./accept-invitation-route";
import { ACTIVE_TEAM_STORAGE_KEY } from "./team-context";
import { teamsApi } from "./teams.api";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("./teams.api", async (load) => {
  const original = await load<typeof import("./teams.api")>();
  return { ...original, teamsApi: { ...original.teamsApi, acceptInvitation: vi.fn() } };
});

const preflight = {
  team: { id: "team-2", name: "Acme Team" },
  role: "member",
  status: "pending",
  expires_at: "2026-08-05T12:00:00Z",
  email_matches_current_user: true,
  can_accept: true,
};

function jsonResponse(data: unknown): Response {
  return Response.json({ code: 200, message: "OK", data });
}

function renderInvitation() {
  return render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <MemoryRouter initialEntries={["/invitations/accept?token=secret"]}>
        <Routes>
          <Route path="/invitations/accept" element={<AcceptInvitationRoute />} />
          <Route path="/" element={<div>overview</div>} />
          <Route path="/login" element={<div>login</div>} />
          <Route path="/signup" element={<div>signup</div>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  localStorage.clear();
  vi.mocked(useAuth).mockReturnValue({
    user: {
      id: "user-1",
      email: "invitee@example.com",
      display_name: "Invitee",
      platform_role: "user",
    },
    isLoading: false,
  } as ReturnType<typeof useAuth>);
  vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse(preflight));
});

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

it("accepts the token, refreshes teams, and opens the joined team", async () => {
  const user = userEvent.setup();
  vi.mocked(teamsApi.acceptInvitation).mockResolvedValue({
    member: { id: "member-1", user_id: "user-1", team_id: "team-2", role: "member" },
  });
  renderInvitation();
  expect(await screen.findByText("Acme Team")).toBeInTheDocument();
  expect(screen.getByText("Member")).toBeInTheDocument();
  expect(screen.getByText(/Aug 5, 2026/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Accept invitation" }));
  await waitFor(() => expect(screen.getByText("overview")).toBeInTheDocument());
  expect(teamsApi.acceptInvitation).toHaveBeenCalledWith("secret");
  expect(localStorage.getItem(ACTIVE_TEAM_STORAGE_KEY)).toBe("team-2");
});

it("shows an email mismatch immediately and does not offer acceptance", async () => {
  vi.mocked(globalThis.fetch).mockResolvedValue(
    jsonResponse({
      ...preflight,
      status: "email_mismatch",
      email_matches_current_user: false,
      can_accept: false,
    }),
  );

  renderInvitation();

  expect(await screen.findByText("Acme Team")).toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("different email address");
  expect(screen.queryByRole("button", { name: "Accept invitation" })).not.toBeInTheDocument();
});

it("shows invitation details and authentication actions before login", async () => {
  vi.mocked(useAuth).mockReturnValue({ user: null, isLoading: false } as ReturnType<
    typeof useAuth
  >);
  vi.mocked(globalThis.fetch).mockResolvedValue(
    jsonResponse({
      ...preflight,
      email_matches_current_user: null,
      can_accept: false,
    }),
  );

  renderInvitation();

  expect(await screen.findByText("Acme Team")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Log in" })).toHaveAttribute(
    "href",
    "/login?invitation_token=secret",
  );
  expect(screen.getByRole("link", { name: "Create account" })).toHaveAttribute(
    "href",
    "/signup?invitation_token=secret",
  );
});
