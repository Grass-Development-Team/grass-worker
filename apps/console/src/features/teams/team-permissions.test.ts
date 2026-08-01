import { describe, expect, it } from "vite-plus/test";

import {
  canContributeToProjects,
  canCreateProject,
  canEditTeam,
  canManageMembers,
  canManageProjectLifecycle,
  canViewTeamAudit,
  canViewTeamSettings,
} from "./team-permissions";

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

  it("keeps viewers read-only while members can contribute", () => {
    expect(canCreateProject("member")).toBe(true);
    expect(canContributeToProjects("member")).toBe(true);
    expect(canCreateProject("viewer")).toBe(false);
    expect(canContributeToProjects("viewer")).toBe(false);
  });

  it("limits lifecycle management and team audit to owners and admins", () => {
    for (const role of ["owner", "admin"] as const) {
      expect(canManageProjectLifecycle(role)).toBe(true);
      expect(canViewTeamAudit(role)).toBe(true);
    }
    for (const role of ["member", "viewer"] as const) {
      expect(canManageProjectLifecycle(role)).toBe(false);
      expect(canViewTeamAudit(role)).toBe(false);
    }
  });
});
