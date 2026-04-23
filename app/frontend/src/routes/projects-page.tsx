import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { useNavigate, useOutletContext } from "react-router-dom";
import { ApiError } from "@/api/client";
import {
  createProject,
  getProjects,
  projectsQueryKey,
  type Project,
} from "@/api/projects";
import { ConsolePageHeader } from "@/components/console/console-page-header";
import { CreateProjectCard } from "@/components/projects/create-project-card";
import { ProjectList } from "@/components/projects/project-list";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import type { ProtectedOutletContext } from "./protected-route";

function sortProjects(projects: Project[]) {
  return [...projects].sort((left, right) => {
    const statusRank = (status: Project["status"]) => {
      if (status === "active") return 0;
      if (status === "archived") return 1;
      return 2;
    };

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

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }

  return fallback;
}

export function ProjectsPage() {
  const { currentUser } = useOutletContext<ProtectedOutletContext>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [createResetToken, setCreateResetToken] = React.useState(0);
  const projectsQuery = useQuery({
    queryKey: projectsQueryKey,
    queryFn: getProjects,
  });
  const createProjectMutation = useMutation({
    mutationFn: ({ name, slug }: { name: string; slug: string }) =>
      createProject({ name, slug }),
    onSuccess: async (project) => {
      queryClient.setQueryData<Project[]>(projectsQueryKey, (current) =>
        mergeProjects(current ?? [], [project]),
      );
      setCreateResetToken((current) => current + 1);
      await projectsQuery.refetch();
    },
  });

  const projects = mergeProjects(projectsQuery.data ?? []);
  const createError = createProjectMutation.isError
    ? errorMessage(createProjectMutation.error, "Unable to create project")
    : null;

  return (
    <div className="space-y-6">
      <ConsolePageHeader
        actions={
          <Button
            disabled={projectsQuery.isPending}
            onClick={() => void projectsQuery.refetch()}
            type="button"
            variant="outline"
          >
            {projectsQuery.isPending ? "Refreshing..." : "Refresh projects"}
          </Button>
        }
        description={`Review the deployment workspaces available to ${currentUser.email}.`}
        eyebrow="Ready mode workspace"
        title="Projects"
      />

      {projectsQuery.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Unable to load projects</AlertTitle>
          <AlertDescription>
            {errorMessage(projectsQuery.error, "The projects list request failed.")}
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(320px,1fr)]">
        <ProjectList
          isLoading={projectsQuery.isPending}
          onViewProject={(projectId) => void navigate(`/projects/${projectId}`)}
          projects={projects}
        />
        <CreateProjectCard
          error={createError}
          isCreating={createProjectMutation.isPending}
          onCreateProject={(input) => createProjectMutation.mutate(input)}
          onResetError={() => createProjectMutation.reset()}
          resetToken={createResetToken}
        />
      </div>
    </div>
  );
}
