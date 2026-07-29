import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { it, vi } from "vitest";

import { useTeam } from "@/features/teams/team-context";

import { TeamAuditRoute } from "./team-audit-route";

vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));
vi.mock("./audit-events-table", () => ({ AuditEventsTable: () => <div>audit table</div> }));

it.each(["member", "viewer"] as const)("redirects %s away from team audit", (role) => {
  vi.mocked(useTeam).mockReturnValue({
    activeTeam: { id: "team-1", name: "Acme" },
    activeRole: role,
  } as ReturnType<typeof useTeam>);

  render(
    <MemoryRouter initialEntries={["/settings/audit"]}>
      <Routes>
        <Route path="/settings/audit" element={<TeamAuditRoute />} />
        <Route path="/" element={<div>overview</div>} />
      </Routes>
    </MemoryRouter>,
  );

  expect(screen.getByText("overview")).toBeInTheDocument();
  expect(screen.queryByText("audit table")).not.toBeInTheDocument();
});
