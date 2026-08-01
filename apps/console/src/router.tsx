import { lazy, Suspense } from "react";
import { Navigate, Route, Routes, useLocation } from "react-router";

import { useAuth } from "@/features/auth/auth-context";
import { TeamProvider } from "@/features/teams/team-context";
import { TeamSettingsGuard } from "@/features/teams/team-settings-guard";

const AuthLayout = lazy(() =>
  import("@/layouts/auth-layout").then(({ AuthLayout }) => ({ default: AuthLayout })),
);
const AppLayout = lazy(() =>
  import("@/layouts/app-layout").then(({ AppLayout }) => ({ default: AppLayout })),
);
const SetupRoute = lazy(() =>
  import("@/features/setup/setup-route").then(({ SetupRoute }) => ({ default: SetupRoute })),
);
const LoginRoute = lazy(() =>
  import("@/features/auth/login-route").then(({ LoginRoute }) => ({ default: LoginRoute })),
);
const SignupRoute = lazy(() =>
  import("@/features/auth/signup-route").then(({ SignupRoute }) => ({ default: SignupRoute })),
);
const DashboardRoute = lazy(() =>
  import("@/features/dashboard/dashboard-route").then(({ DashboardRoute }) => ({
    default: DashboardRoute,
  })),
);
const TeamSettingsRoute = lazy(() =>
  import("@/features/teams/team-settings-route").then(({ TeamSettingsRoute }) => ({
    default: TeamSettingsRoute,
  })),
);
const TeamMembersRoute = lazy(() =>
  import("@/features/teams/team-members-route").then(({ TeamMembersRoute }) => ({
    default: TeamMembersRoute,
  })),
);
const AcceptInvitationRoute = lazy(() =>
  import("@/features/teams/accept-invitation-route").then(({ AcceptInvitationRoute }) => ({
    default: AcceptInvitationRoute,
  })),
);
const AdminRoute = lazy(() =>
  import("@/features/admin/admin-route").then(({ AdminRoute }) => ({ default: AdminRoute })),
);
const ReviewsPanel = lazy(() =>
  import("@/features/admin/components/reviews-panel").then(({ ReviewsPanel }) => ({
    default: ReviewsPanel,
  })),
);
const ProjectsPanel = lazy(() =>
  import("@/features/admin/components/projects-panel").then(({ ProjectsPanel }) => ({
    default: ProjectsPanel,
  })),
);
const ProjectGovernancePage = lazy(() =>
  import("@/features/admin/components/project-governance-page").then(
    ({ ProjectGovernancePage }) => ({ default: ProjectGovernancePage }),
  ),
);
const NodesPanel = lazy(() =>
  import("@/features/admin/components/nodes-panel").then(({ NodesPanel }) => ({
    default: NodesPanel,
  })),
);
const HostSourcesPanel = lazy(() =>
  import("@/features/admin/components/host-sources-panel").then(({ HostSourcesPanel }) => ({
    default: HostSourcesPanel,
  })),
);
const QuotaPlansPanel = lazy(() =>
  import("@/features/admin/components/quota-plans-panel").then(({ QuotaPlansPanel }) => ({
    default: QuotaPlansPanel,
  })),
);
const TeamGroupsPanel = lazy(() =>
  import("@/features/admin/components/team-groups-panel").then(({ TeamGroupsPanel }) => ({
    default: TeamGroupsPanel,
  })),
);
const UsersPanel = lazy(() =>
  import("@/features/admin/components/users-panel").then(({ UsersPanel }) => ({
    default: UsersPanel,
  })),
);
const TeamsPanel = lazy(() =>
  import("@/features/admin/components/teams-panel").then(({ TeamsPanel }) => ({
    default: TeamsPanel,
  })),
);
const SettingsPanel = lazy(() =>
  import("@/features/admin/components/settings-panel").then(({ SettingsPanel }) => ({
    default: SettingsPanel,
  })),
);
const AuditEventsTable = lazy(() =>
  import("@/features/audit/audit-events-table").then(({ AuditEventsTable }) => ({
    default: AuditEventsTable,
  })),
);
const QuotaRoute = lazy(() =>
  import("@/features/quota/quota-route").then(({ QuotaRoute }) => ({ default: QuotaRoute })),
);
const ProjectsRoute = lazy(() =>
  import("@/features/projects/projects-route").then(({ ProjectsRoute }) => ({
    default: ProjectsRoute,
  })),
);
const ProjectLayout = lazy(() =>
  import("@/features/projects/project-layout").then(({ ProjectLayout }) => ({
    default: ProjectLayout,
  })),
);
const ProjectOverviewRoute = lazy(() =>
  import("@/features/projects/project-overview-route").then(({ ProjectOverviewRoute }) => ({
    default: ProjectOverviewRoute,
  })),
);
const ProjectDeploymentsRoute = lazy(() =>
  import("@/features/projects/project-deployments-route").then(({ ProjectDeploymentsRoute }) => ({
    default: ProjectDeploymentsRoute,
  })),
);
const ProjectDomainsRoute = lazy(() =>
  import("@/features/projects/project-domains-route").then(({ ProjectDomainsRoute }) => ({
    default: ProjectDomainsRoute,
  })),
);
const ProjectSettingsRoute = lazy(() =>
  import("@/features/projects/project-settings-route").then(({ ProjectSettingsRoute }) => ({
    default: ProjectSettingsRoute,
  })),
);
const ProjectSettingsBuildRoute = lazy(() =>
  import("@/features/projects/project-settings-build-route").then(
    ({ ProjectSettingsBuildRoute }) => ({ default: ProjectSettingsBuildRoute }),
  ),
);
const DeploymentDetailRoute = lazy(() =>
  import("@/features/deployments/deployment-detail-route").then(({ DeploymentDetailRoute }) => ({
    default: DeploymentDetailRoute,
  })),
);
const TeamAuditRoute = lazy(() =>
  import("@/features/audit/team-audit-route").then(({ TeamAuditRoute }) => ({
    default: TeamAuditRoute,
  })),
);
const NotificationsRoute = lazy(() =>
  import("@/features/notifications/notifications-route").then(({ NotificationsRoute }) => ({
    default: NotificationsRoute,
  })),
);

function RouteLoadingFallback() {
  return (
    <div
      className="flex min-h-32 items-center justify-center text-sm text-muted-foreground"
      role="status"
    >
      Loading page...
    </div>
  );
}

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
    <Suspense fallback={<RouteLoadingFallback />}>
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
          <Route path="/notifications" element={<NotificationsRoute />} />
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
            <Route path="projects/:projectId" element={<ProjectGovernancePage />} />
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
    </Suspense>
  );
}
