import type { TeamRole } from "./teams.api";

export const canViewTeamSettings = (_role: TeamRole) => true;
export const canEditTeam = (role: TeamRole) => role === "owner";
export const canManageMembers = (role: TeamRole) => role === "owner" || role === "admin";
