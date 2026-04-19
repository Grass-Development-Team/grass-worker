import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { useLocation, useNavigate, useOutletContext } from "react-router-dom";
import { ApiError } from "@/api/client";
import { currentUserQueryKey, logout } from "@/api/auth";
import { createProject, getProjects, projectsQueryKey, type Project } from "@/api/projects";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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

function deriveSlug(value: string) {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .replace(/-{2,}/g, "-");
}

function sortProjects(projects: Project[]) {
  return [...projects].sort((left, right) => {
    const statusRank = (status: Project["status"]) => (status === "active" ? 0 : 1);

    return (
      statusRank(left.status) - statusRank(right.status) ||
      new Date(right.updated_at).getTime() - new Date(left.updated_at).getTime() ||
      left.name.localeCompare(right.name)
    );
  });
}

function mergeProjects(projects: Project[], createdProjects: Project[] = []) {
  const byId = new Map<string, Project>();

  for (const project of projects) {
    byId.set(project.id, project);
  }

  for (const project of createdProjects) {
    byId.set(project.id, project);
  }

  return sortProjects([...byId.values()]);
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
  const [projectName, setProjectName] = React.useState("");
  const [projectSlug, setProjectSlug] = React.useState("");
  const [slugTouched, setSlugTouched] = React.useState(false);
  const [createValidationError, setCreateValidationError] = React.useState<string | null>(null);
  const [createdProjects, setCreatedProjects] = React.useState<Project[]>([]);
  const projectsQuery = useQuery({
    queryKey: projectsQueryKey,
    queryFn: getProjects,
  });
  const createProjectMutation = useMutation({
    mutationFn: ({ name, slug }: { name: string; slug: string }) =>
      createProject({ name, slug }),
    onSuccess: async (project) => {
      setProjectName("");
      setProjectSlug("");
      setSlugTouched(false);
      setCreateValidationError(null);
      setCreatedProjects((current) => mergeProjects(current, [project]));
      void projectsQuery.refetch();
    },
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

  const projects = mergeProjects(projectsQuery.data ?? [], createdProjects);
  const activeProjects = projects.filter((project) => project.status === "active");
  const archivedProjects = projects.filter((project) => project.status === "archived");
  const createError =
    createValidationError ??
    (createProjectMutation.isError
      ? createProjectMutation.error instanceof ApiError ||
        createProjectMutation.error instanceof Error
        ? createProjectMutation.error.message
        : "Unable to create project"
      : null);

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
                    Use the create form to provision the first project for this workspace.
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
              <CardTitle>Create project</CardTitle>
              <CardDescription>
                Provision a deployment workspace and it will appear in the inventory immediately.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <form
                className="space-y-4"
                onSubmit={(event) => {
                  event.preventDefault();

                  const name = projectName.trim();
                  const slug = projectSlug.trim();

                  if (!name) {
                    setCreateValidationError("Project name is required");
                    return;
                  }

                  if (!slug) {
                    setCreateValidationError("Project slug is required");
                    return;
                  }

                  setCreateValidationError(null);
                  createProjectMutation.mutate({ name, slug });
                }}
              >
                <div className="space-y-2">
                  <Label htmlFor="project-name">Project name</Label>
                  <Input
                    id="project-name"
                    onChange={(event) => {
                      const value = event.target.value;
                      setProjectName(value);
                      setCreateValidationError(null);
                      createProjectMutation.reset();
                      if (!slugTouched) {
                        setProjectSlug(deriveSlug(value));
                      }
                    }}
                    placeholder="Docs Site"
                    value={projectName}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="project-slug">Project slug</Label>
                  <Input
                    id="project-slug"
                    onChange={(event) => {
                      setSlugTouched(true);
                      setProjectSlug(deriveSlug(event.target.value));
                      setCreateValidationError(null);
                      createProjectMutation.reset();
                    }}
                    placeholder="docs-site"
                    value={projectSlug}
                  />
                </div>
                {createError ? (
                  <Alert variant="destructive">
                    <AlertTitle>Project creation failed</AlertTitle>
                    <AlertDescription>{createError}</AlertDescription>
                  </Alert>
                ) : null}
                <Button className="w-full" disabled={createProjectMutation.isPending} type="submit">
                  {createProjectMutation.isPending ? "Creating project..." : "Create project"}
                </Button>
              </form>
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
            </CardContent>
          </Card>
        </div>
      </div>
    </main>
  );
}
