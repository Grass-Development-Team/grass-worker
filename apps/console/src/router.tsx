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
import { ReviewsPanel } from "@/features/admin/components/reviews-panel";
import { ProjectsPanel } from "@/features/admin/components/projects-panel";
import { NodesPanel } from "@/features/admin/components/nodes-panel";
import { HostSourcesPanel } from "@/features/admin/components/host-sources-panel";
import { QuotaPlansPanel } from "@/features/admin/components/quota-plans-panel";
import { TeamGroupsPanel } from "@/features/admin/components/team-groups-panel";
import { UsersPanel } from "@/features/admin/components/users-panel";
import { TeamsPanel } from "@/features/admin/components/teams-panel";
import { SettingsPanel } from "@/features/admin/components/settings-panel";
import { AuditEventsTable } from "@/features/audit/audit-events-table";
import { QuotaRoute } from "@/features/quota/quota-route";
import { ProjectsRoute } from "@/features/projects/projects-route";
import { ProjectLayout } from "@/features/projects/project-layout";
import { ProjectOverviewRoute } from "@/features/projects/project-overview-route";
import { ProjectDeploymentsRoute } from "@/features/projects/project-deployments-route";
import { ProjectDomainsRoute } from "@/features/projects/project-domains-route";
import { ProjectSettingsRoute } from "@/features/projects/project-settings-route";
import { ProjectSettingsBuildRoute } from "@/features/projects/project-settings-build-route";
import { DeploymentDetailRoute } from "@/features/deployments/deployment-detail-route";
import { TeamAuditRoute } from "@/features/audit/team-audit-route";

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

function PlatformAdminRoute({ children }: { children: React.ReactNode }) {
  const { user } = useAuth();
  return user?.platform_role === "admin" ? <>{children}</> : <Navigate to="/" replace />;
}

export function Router() {
  return (
    <Routes>
      <Route element={<AuthLayout />}>
        <Route path="/setup" element={<SetupRoute />} />
        <Route path="/invitations/accept" element={<AcceptInvitationRoute />} />
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
        <Route path="/dashboard" element={<Navigate to="/" replace />} />
        <Route path="/quota" element={<QuotaRoute />} />
        <Route path="/projects" element={<ProjectsRoute />} />
        <Route path="/projects/:projectId" element={<ProjectLayout />}>
          <Route index element={<ProjectOverviewRoute />} />
          <Route path="deployments" element={<ProjectDeploymentsRoute />} />
          <Route path="deployments/:deploymentId" element={<DeploymentDetailRoute />} />
          <Route path="domains" element={<ProjectDomainsRoute />} />
          <Route path="settings" element={<ProjectSettingsRoute />} />
          <Route path="settings/build-and-deployment" element={<ProjectSettingsBuildRoute />} />
        </Route>
        <Route
          path="/admin"
          element={
            <PlatformAdminRoute>
              <AdminRoute />
            </PlatformAdminRoute>
          }
        >
          <Route index element={<Navigate to="/admin/reviews" replace />} />
          <Route path="reviews" element={<ReviewsPanel />} />
          <Route path="projects" element={<ProjectsPanel />} />
          <Route path="nodes" element={<NodesPanel />} />
          <Route path="host-sources" element={<HostSourcesPanel />} />
          <Route path="quota-plans" element={<QuotaPlansPanel />} />
          <Route path="team-groups" element={<TeamGroupsPanel />} />
          <Route path="users" element={<UsersPanel />} />
          <Route path="teams" element={<TeamsPanel />} />
          <Route path="settings" element={<SettingsPanel />} />
          <Route path="audit" element={<AuditEventsTable />} />
        </Route>
        <Route element={<TeamSettingsGuard />}>
          <Route path="/settings/team" element={<TeamSettingsRoute />} />
          <Route path="/settings/members" element={<TeamMembersRoute />} />
          <Route path="/settings/audit" element={<TeamAuditRoute />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
