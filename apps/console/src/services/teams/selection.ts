import type { Team } from "@/services/teams/index";

export function selectActiveTeam(teams: Team[], persistedTeamId: string | null): Team | null {
  if (teams.length === 0) {
    return null;
  }

  return (
    teams.find((team) => team.id === persistedTeamId) ??
    teams.find((team) => team.kind === "personal") ??
    teams[0]
  );
}
