import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { expect, it, vi } from "vite-plus/test";

import { AcceptInvitationRoute } from "./accept-invitation-route";
import { useTeam } from "./team-context";
import { teamsApi } from "./teams.api";

vi.mock("./team-context", () => ({ useTeam: vi.fn() }));
vi.mock("./teams.api", async (load) => {
  const original = await load<typeof import("./teams.api")>();
  return { ...original, teamsApi: { ...original.teamsApi, acceptInvitation: vi.fn() } };
});

it("accepts the token, refreshes teams, and opens the joined team", async () => {
  const user = userEvent.setup();
  const refreshTeams = vi.fn().mockResolvedValue(undefined);
  const selectTeam = vi.fn();
  vi.mocked(useTeam).mockReturnValue({ refreshTeams, selectTeam } as ReturnType<typeof useTeam>);
  vi.mocked(teamsApi.acceptInvitation).mockResolvedValue({
    member: { id: "member-1", user_id: "user-1", team_id: "team-2", role: "member" },
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter initialEntries={["/invitations/accept?token=secret"]}>
        <Routes>
          <Route path="/invitations/accept" element={<AcceptInvitationRoute />} />
          <Route path="/" element={<div>overview</div>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  await user.click(screen.getByRole("button", { name: "Accept invitation" }));
  await waitFor(() => expect(screen.getByText("overview")).toBeInTheDocument());
  expect(teamsApi.acceptInvitation).toHaveBeenCalledWith("secret");
  expect(refreshTeams).toHaveBeenCalled();
  expect(selectTeam).toHaveBeenCalledWith("team-2");
});
