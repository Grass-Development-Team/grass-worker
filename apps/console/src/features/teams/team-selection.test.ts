import { describe, expect, it } from "vite-plus/test";

import type { Team } from "./teams.api";
import { selectActiveTeam } from "./team-selection";

const team = (id: string, kind: Team["kind"]): Team => ({
  id,
  kind,
  slug: id,
  name: id,
  owner_user_id: "user-1",
  group_id: null,
});

describe("selectActiveTeam", () => {
  const teams = [team("shared", "team"), team("personal", "personal")];

  it("restores a persisted team that is still available", () => {
    expect(selectActiveTeam(teams, "shared")?.id).toBe("shared");
  });

  it("falls back to the personal team when the persisted team is invalid", () => {
    expect(selectActiveTeam(teams, "removed")?.id).toBe("personal");
  });

  it("falls back to the first team when no personal team exists", () => {
    expect(selectActiveTeam([team("shared", "team")], null)?.id).toBe("shared");
  });

  it("returns null when the user has no teams", () => {
    expect(selectActiveTeam([], "removed")).toBeNull();
  });
});
