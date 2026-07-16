import { Navigate, Route, Routes, useLocation } from "react-router";

import { AuthLayout } from "@/layouts/auth-layout";
import { AppLayout } from "@/layouts/app-layout";
import { SetupRoute } from "@/features/setup/setup-route";
import { LoginRoute } from "@/features/auth/login-route";
import { SignupRoute } from "@/features/auth/signup-route";
import { DashboardRoute } from "@/features/dashboard/dashboard-route";
import { useAuth } from "@/features/auth/auth-context";
import { TeamProvider } from "@/features/teams/team-context";
import { TeamSettingsGuard } from "@/features/teams/team-settings-guard";
import { TeamSettingsRoute } from "@/features/teams/team-settings-route";
import { TeamMembersRoute } from "@/features/teams/team-members-route";
import { AcceptInvitationRoute } from "@/features/teams/accept-invitation-route";
import { AdminRoute } from "@/features/admin/admin-route";

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { user, isLoading } = useAuth();
  const location = useLocation();

  if (isLoading) {
    return null;
  }

  if (!user) {
    return (
      <Navigate to="/login" replace state={{ from: `${location.pathname}${location.search}` }} />
    );
  }

  return <>{children}</>;
}

function GuestRoute({ children }: { children: React.ReactNode }) {
  const { user, isLoading } = useAuth();
  if (isLoading) return null;
  return user ? <Navigate to="/" replace /> : <>{children}</>;
}

export function Router() {
  return (
    <Routes>
      <Route element={<AuthLayout />}>
        <Route path="/setup" element={<SetupRoute />} />
        <Route
          path="/login"
          element={
            <GuestRoute>
              <LoginRoute />
            </GuestRoute>
          }
        />
        <Route
          path="/signup"
          element={
            <GuestRoute>
              <SignupRoute />
            </GuestRoute>
          }
        />
      </Route>
      <Route
        element={
          <ProtectedRoute>
            <TeamProvider>
              <AppLayout />
            </TeamProvider>
          </ProtectedRoute>
        }
      >
        <Route path="/" element={<DashboardRoute />} />
        <Route path="/dashboard" element={<DashboardRoute />} />
        <Route path="/admin" element={<AdminRoute />} />
        <Route path="/invitations/accept" element={<AcceptInvitationRoute />} />
        <Route element={<TeamSettingsGuard />}>
          <Route path="/settings/team" element={<TeamSettingsRoute />} />
          <Route path="/settings/members" element={<TeamMembersRoute />} />
        </Route>
      </Route>
    </Routes>
  );
}
