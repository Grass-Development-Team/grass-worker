import { Navigate, Outlet } from "react-router";

import { canViewTeamSettings } from "./team-permissions";
import { useTeam } from "./team-context";

export function TeamSettingsGuard() {
  const { activeRole, isLoading } = useTeam();
  if (isLoading) return null;
  if (!activeRole || !canViewTeamSettings(activeRole)) return <Navigate to="/" replace />;
  return <Outlet />;
}
