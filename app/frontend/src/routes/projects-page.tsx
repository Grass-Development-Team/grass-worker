import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useLocation, useNavigate, useOutletContext } from "react-router-dom";
import { currentUserQueryKey, logout } from "@/api/auth";
import { getProjects, projectsQueryKey } from "@/api/projects";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { ProtectedOutletContext } from "./protected-route";

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function metricValue(value: number) {
  return value.toString().padStart(2, "0");
}

function ProjectMetricCard({
  label,
  value,
  description,
}: {
  label: string;
  value: number;
  description: string;
}) {
  return (
    <Card>
      <CardHeader>
        <CardDescription>{label}</CardDescription>
        <CardTitle>{metricValue(value)}</CardTitle>
      </CardHeader>
      <CardContent className="text-sm text-muted-foreground">
        {description}
      </CardContent>
    </Card>
  );
}

export function ProjectsPage() {
  const { currentUser } = useOutletContext<ProtectedOutletContext>();
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const projectsQuery = useQuery({
    queryKey: projectsQueryKey,
    queryFn: getProjects,
  });
  const signOutMutation = useMutation({
    mutationFn: logout,
    onSuccess: async () => {
      queryClient.setQueryData(currentUserQueryKey, null);
      await navigate(
        `/login?redirect=${encodeURIComponent(
          `${location.pathname}${location.search}`,
        )}`,
        { replace: true },
      );
    },
  });

  const projects = projectsQuery.data ?? [];
  const activeProjects = projects.filter((project) => project.status === "active");
  const archivedProjects = projects.filter((project) => project.status === "archived");

  return (
    <main className="min-h-screen bg-muted/30 px-6 py-10">
      <div className="mx-auto max-w-6xl space-y-6">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
          <div className="space-y-1">
            <p className="text-sm text-muted-foreground">Ready mode workspace</p>
            <h1 className="text-3xl font-semibold tracking-tight">Projects</h1>
            <p className="text-sm text-muted-foreground">
              Review the deployment workspaces available to {currentUser.email}.
            </p>
          </div>
          <div className="flex flex-col gap-3 sm:flex-row">
            <Button
              disabled={projectsQuery.isPending}
              onClick={() => void projectsQuery.refetch()}
              type="button"
              variant="outline"
            >
              {projectsQuery.isPending ? "Refreshing..." : "Refresh projects"}
            </Button>
            <Button
              disabled={signOutMutation.isPending}
              onClick={() => signOutMutation.mutate()}
              type="button"
              variant="outline"
            >
              {signOutMutation.isPending ? "Signing out..." : "Sign out"}
            </Button>
          </div>
        </div>

        <div className="grid gap-4 md:grid-cols-3">
          <ProjectMetricCard
            description="Every project attached to this admin account."
            label="Total"
            value={projects.length}
          />
          <ProjectMetricCard
            description="Projects that can accept new deployments right now."
            label="Active"
            value={activeProjects.length}
          />
          <ProjectMetricCard
            description="Projects kept for history but not actively deployed."
            label="Archived"
            value={archivedProjects.length}
          />
        </div>

        {projectsQuery.isError ? (
          <Alert variant="destructive">
            <AlertTitle>Unable to load projects</AlertTitle>
            <AlertDescription>
              {projectsQuery.error instanceof Error
                ? projectsQuery.error.message
                : "The projects list request failed."}
            </AlertDescription>
          </Alert>
        ) : null}

        <div className="grid gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(320px,1fr)]">
          <Card>
            <CardHeader>
              <CardTitle>Project inventory</CardTitle>
              <CardDescription>
                This list is loaded from the control API and scoped to the signed-in owner.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              {projectsQuery.isPending ? (
                <Card>
                  <CardHeader>
                    <CardTitle>Loading projects</CardTitle>
                    <CardDescription>
                      Fetching the current workspace inventory from the API.
                    </CardDescription>
                  </CardHeader>
                </Card>
              ) : projects.length === 0 ? (
                <Card>
                  <CardHeader>
                    <CardTitle>No projects yet</CardTitle>
                    <CardDescription>
                      The control plane is ready, but this account has not created any deployment
                      workspaces yet.
                    </CardDescription>
                  </CardHeader>
                  <CardContent className="text-sm text-muted-foreground">
                    Project creation can land next without replacing this page. The inventory,
                    session handling, and ready-mode routing are already in place.
                  </CardContent>
                </Card>
              ) : (
                projects.map((project) => (
                  <Card key={project.id}>
                    <CardHeader>
                      <CardDescription>{project.slug}</CardDescription>
                      <CardTitle>{project.name}</CardTitle>
                      <CardDescription>
                        {project.status === "active"
                          ? "Active project"
                          : "Archived project"}
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-3">
                      <div className="space-y-1">
                        <p>Status</p>
                        <p className="font-medium text-foreground">
                          {project.status === "active" ? "Active" : "Archived"}
                        </p>
                      </div>
                      <div className="space-y-1">
                        <p>Created</p>
                        <p className="font-medium text-foreground">
                          {formatTimestamp(project.created_at)}
                        </p>
                      </div>
                      <div className="space-y-1">
                        <p>{project.archived_at ? "Archived" : "Updated"}</p>
                        <p className="font-medium text-foreground">
                          {formatTimestamp(project.archived_at ?? project.updated_at)}
                        </p>
                      </div>
                    </CardContent>
                  </Card>
                ))
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Session</CardTitle>
              <CardDescription>
                The ready-mode console is authenticated with the current `HttpOnly` session cookie.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <Card>
                <CardHeader>
                  <CardDescription>Signed in as</CardDescription>
                  <CardTitle>{currentUser.email}</CardTitle>
                </CardHeader>
                <CardContent className="space-y-3 text-sm text-muted-foreground">
                  <div className="flex items-center justify-between gap-3">
                    <span>Role</span>
                    <span className="font-medium text-foreground">
                      {currentUser.is_admin ? "Administrator" : "User"}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span>Bootstrap account</span>
                    <span className="font-medium text-foreground">
                      {currentUser.is_initial_admin ? "Initial admin" : "Standard account"}
                    </span>
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardTitle>Next step</CardTitle>
                  <CardDescription>
                    The dashboard is ready for project creation and detail views to plug in next.
                  </CardDescription>
                </CardHeader>
              </Card>
            </CardContent>
          </Card>
        </div>
      </div>
    </main>
  );
}
