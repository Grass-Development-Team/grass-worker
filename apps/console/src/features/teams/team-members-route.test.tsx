import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { TeamMembersRoute } from "./team-members-route";
import { useTeam } from "./team-context";
import { teamsApi, type TeamMember } from "./teams.api";

vi.mock("./team-context", () => ({
  teamKeys: { members: (teamId: string) => ["teams", teamId, "members"] },
  useTeam: vi.fn(),
}));
vi.mock("./teams.api", async (load) => {
  const original = await load<typeof import("./teams.api")>();
  return {
    ...original,
    teamsApi: {
      ...original.teamsApi,
      listMembers: vi.fn(),
      invitationCandidates: vi.fn(),
      inviteMember: vi.fn(),
      updateMemberRole: vi.fn(),
      removeMember: vi.fn(),
    },
  };
});

const member: TeamMember = {
  id: "member-2",
  user_id: "user-2",
  email: "member@example.com",
  display_name: "Member",
  role: "member",
  joined_at: "2026-07-16T08:00:00Z",
};

function renderRoute() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <TeamMembersRoute />
    </QueryClientProvider>,
  );
}

describe("TeamMembersRoute", () => {
  beforeEach(() => {
    vi.mocked(useTeam).mockReturnValue({
      activeTeam: {
        id: "team-1",
        slug: "team",
        name: "Team",
        kind: "team",
        owner_user_id: "user-1",
        group_id: null,
      },
      activeRole: "owner",
    } as ReturnType<typeof useTeam>);
    vi.mocked(teamsApi.listMembers).mockResolvedValue({ members: [member] });
    vi.mocked(teamsApi.invitationCandidates).mockResolvedValue({ candidates: [] });
  });

  afterEach(() => vi.clearAllMocks());

  it("shows a role update failure", async () => {
    const user = userEvent.setup();
    vi.mocked(teamsApi.updateMemberRole).mockRejectedValue(new Error("Role update failed"));
    renderRoute();

    await user.click(await screen.findByRole("combobox", { name: "Role for member@example.com" }));
    await user.click(screen.getByRole("option", { name: "admin" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Role update failed");
  });

  it("clears a failed invitation before reopening the dialog", async () => {
    const user = userEvent.setup();
    vi.mocked(teamsApi.invitationCandidates).mockResolvedValue({
      candidates: [
        {
          kind: "user",
          user_id: "user-3",
          email: "new@example.com",
          display_name: "New User",
        },
      ],
    });
    vi.mocked(teamsApi.inviteMember).mockRejectedValue(new Error("Invitation failed"));
    renderRoute();

    await user.click(screen.getByRole("button", { name: "Invite member" }));
    await user.type(screen.getByLabelText("Email"), "new@example.com");
    await user.click(await screen.findByRole("option", { name: /New User/ }));
    await user.click(screen.getByRole("button", { name: "Create invitation" }));
    expect(await screen.findByText("Invitation failed")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Close" }));
    await user.click(screen.getByRole("button", { name: "Invite member" }));

    expect(screen.queryByText("Invitation failed")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Email")).toHaveValue("");
  });

  it("searches and selects a registered user before creating an invitation", async () => {
    const user = userEvent.setup();
    vi.mocked(teamsApi.invitationCandidates).mockResolvedValue({
      candidates: [
        {
          kind: "user",
          user_id: "user-3",
          email: "alice@example.com",
          display_name: "Alice",
        },
      ],
    });
    vi.mocked(teamsApi.inviteMember).mockResolvedValue({
      invitation: {
        id: "invitation-1",
        team_id: "team-1",
        email: "alice@example.com",
        role: "member",
        status: "pending",
        expires_at: "2026-08-13T08:00:00Z",
        token: "secret",
      },
    });
    renderRoute();

    await user.click(screen.getByRole("button", { name: "Invite member" }));
    await user.type(screen.getByLabelText("Email"), "ali");
    await user.click(await screen.findByRole("option", { name: /Alice.*alice@example.com/ }));
    await user.click(screen.getByRole("button", { name: "Create invitation" }));

    expect(teamsApi.invitationCandidates).toHaveBeenCalledWith("team-1", "ali");
    expect(teamsApi.inviteMember).toHaveBeenCalledWith("team-1", {
      email: "alice@example.com",
      role: "member",
    });
  });

  it("offers an open-registration email candidate as Invite User", async () => {
    const user = userEvent.setup();
    vi.mocked(teamsApi.invitationCandidates).mockResolvedValue({
      candidates: [
        {
          kind: "email",
          user_id: null,
          email: "new-user@example.com",
          display_name: null,
        },
      ],
    });
    renderRoute();

    await user.click(screen.getByRole("button", { name: "Invite member" }));
    await user.type(screen.getByLabelText("Email"), "new-user@example.com");

    expect(await screen.findByText("Invite User")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /new-user@example.com.*Invite User/ })).toBeVisible();
  });

  it("reports a missing user and disables submission when no candidate is available", async () => {
    const user = userEvent.setup();
    renderRoute();

    await user.click(screen.getByRole("button", { name: "Invite member" }));
    await user.type(screen.getByLabelText("Email"), "missing@example.com");

    expect(await screen.findByText("User does not exist.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create invitation" })).toBeDisabled();
  });
});
