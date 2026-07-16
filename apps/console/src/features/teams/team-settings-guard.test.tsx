import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { describe, expect, it, vi } from "vite-plus/test";

import { useTeam } from "./team-context";
import { TeamSettingsGuard } from "./team-settings-guard";

vi.mock("./team-context", () => ({ useTeam: vi.fn() }));

function renderGuard(role: "owner" | "admin" | "member" | "viewer") {
  vi.mocked(useTeam).mockReturnValue({ activeRole: role, isLoading: false } as ReturnType<
    typeof useTeam
  >);
  render(
    <MemoryRouter initialEntries={["/settings/team"]}>
      <Routes>
        <Route element={<TeamSettingsGuard />}>
          <Route path="/settings/team" element={<div>settings</div>} />
        </Route>
        <Route path="/" element={<div>overview</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("TeamSettingsGuard", () => {
  it.each(["owner", "admin", "member", "viewer"] as const)("allows %s", (role) => {
    renderGuard(role);
    expect(screen.getByText("settings")).toBeInTheDocument();
  });
});
