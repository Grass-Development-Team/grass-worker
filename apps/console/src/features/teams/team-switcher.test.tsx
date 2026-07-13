import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import type { Team } from "./teams.api";
import { TeamSwitcher } from "./team-switcher";
import { useTeam } from "./team-context";

vi.mock("./team-context", () => ({ useTeam: vi.fn() }));

const personal: Team = {
  id: "personal",
  slug: "personal",
  name: "Personal",
  kind: "personal",
  owner_user_id: "user-1",
  group_id: null,
};
const shared: Team = { ...personal, id: "shared", slug: "acme", name: "Acme", kind: "team" };

function mockTeamContext(overrides: Record<string, unknown> = {}) {
  vi.mocked(useTeam).mockReturnValue({
    teams: [personal, shared],
    activeTeam: personal,
    isLoading: false,
    error: null,
    selectTeam: vi.fn(),
    createTeam: vi.fn().mockResolvedValue(shared),
    refreshTeams: vi.fn(),
    ...overrides,
  });
}

describe("TeamSwitcher", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("shows and switches the active team", async () => {
    const user = userEvent.setup();
    const selectTeam = vi.fn();
    mockTeamContext({ selectTeam });

    render(<TeamSwitcher />);
    await user.click(screen.getByRole("button", { name: /personal/i }));
    await user.click(screen.getByRole("menuitem", { name: /acme/i }));

    expect(selectTeam).toHaveBeenCalledWith("shared");
  });

  it("opens the create team dialog from the switcher", async () => {
    const user = userEvent.setup();
    mockTeamContext();

    render(<TeamSwitcher />);
    await user.click(screen.getByRole("button", { name: /personal/i }));
    await user.click(screen.getByRole("menuitem", { name: /create team/i }));

    expect(screen.getByRole("dialog", { name: "Create team" })).toBeInTheDocument();
  });

  it("creates a team from the open dialog", async () => {
    const user = userEvent.setup();
    const createTeam = vi.fn().mockResolvedValue(shared);
    mockTeamContext({ createTeam });

    render(<TeamSwitcher />);
    await user.click(screen.getByRole("button", { name: /personal/i }));
    await user.click(screen.getByRole("menuitem", { name: /create team/i }));
    await user.type(screen.getByLabelText("Team name"), "Acme");
    await user.type(screen.getByLabelText("Team slug"), "acme");
    await user.click(screen.getByRole("button", { name: "Create team" }));

    await waitFor(() => expect(createTeam).toHaveBeenCalledWith({ name: "Acme", slug: "acme" }));
  });
});
