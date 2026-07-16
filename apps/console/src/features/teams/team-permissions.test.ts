import { describe, expect, it } from "vite-plus/test";

import { canEditTeam, canManageMembers, canViewTeamSettings } from "./team-permissions";

describe("team permissions", () => {
  it.each(["owner", "admin", "member", "viewer"] as const)(
    "allows %s to view team settings",
    (role) => {
      expect(canViewTeamSettings(role)).toBe(true);
    },
  );

  it("only allows owners to edit the team", () => {
    expect(canEditTeam("owner")).toBe(true);
    expect(canEditTeam("admin")).toBe(false);
  });

  it("allows owners and admins to manage members", () => {
    expect(canManageMembers("owner")).toBe(true);
    expect(canManageMembers("admin")).toBe(true);
    expect(canManageMembers("member")).toBe(false);
  });
});
