import { useQuery } from "@tanstack/react-query";
import { ArchiveIcon } from "lucide-react";
import { Outlet, useOutletContext, useParams } from "react-router";

import { Skeleton } from "@/components/ui/skeleton";

import { projectsApi, type Project } from "./projects.api";
import type { TeamRole } from "../teams/teams.api";

export interface ProjectOutletContext {
  project: Project;
  role: TeamRole;
}

/** Access the current project from any route nested under ProjectLayout. */
export function useProject(): ProjectOutletContext {
  return useOutletContext<ProjectOutletContext>();
}

export function ProjectLayout() {
  const { projectId } = useParams<{ projectId: string }>();

  const projectQuery = useQuery({
    queryKey: ["project", projectId],
    queryFn: () => projectsApi.get(projectId as string),
    enabled: Boolean(projectId),
  });

  if (projectQuery.isLoading) {
    return <Skeleton className="h-96 w-full" aria-busy="true" />;
  }
  if (projectQuery.isError || !projectQuery.data) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {projectQuery.error instanceof Error
          ? projectQuery.error.message
          : "Unable to load this project."}
      </p>
    );
  }

  const { project, role } = projectQuery.data;

  return (
    <div className="flex flex-1 flex-col gap-6">
      {project.archived_at && (
        <p className="flex items-center gap-2 rounded-md border bg-muted/40 px-4 py-2.5 text-sm text-muted-foreground">
          <ArchiveIcon className="size-4" />
          This project is archived. Deployments are paused until it is unarchived in Settings.
        </p>
      )}
      <Outlet context={{ project, role: role as TeamRole } satisfies ProjectOutletContext} />
    </div>
  );
}
