import { Navigate, Outlet, useOutletContext } from "react-router-dom";
import type { ProtectedOutletContext } from "./protected-route";

export function AdminRoute() {
  const context = useOutletContext<ProtectedOutletContext>();

  if (!context.currentUser.is_admin) {
    return <Navigate replace to="/projects" />;
  }

  return <Outlet context={context} />;
}
