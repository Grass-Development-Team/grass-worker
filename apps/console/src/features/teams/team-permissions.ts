import type { TeamRole } from "./teams.api";

const isTeamAdmin = (role: TeamRole) => role === "owner" || role === "admin";

export const canViewTeamSettings = (_role: TeamRole) => true;
export const canEditTeam = (role: TeamRole) => role === "owner";
export const canManageMembers = isTeamAdmin;
export const canCreateProject = (role: TeamRole) => role !== "viewer";
export const canContributeToProjects = (role: TeamRole) => role !== "viewer";
export const canManageProjectLifecycle = isTeamAdmin;
export const canViewTeamAudit = isTeamAdmin;
