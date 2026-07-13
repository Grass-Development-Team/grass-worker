import type { TeamRole } from "./teams.api";

export const canViewTeamSettings = (role: TeamRole) => role === "owner" || role === "admin";
export const canEditTeam = (role: TeamRole) => role === "owner";
export const canManageMembers = canViewTeamSettings;
