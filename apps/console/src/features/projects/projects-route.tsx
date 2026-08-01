import { useQuery } from "@tanstack/react-query";
import { FolderGitIcon, PlusIcon } from "lucide-react";
import { Link } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useTeam } from "@/features/teams/team-context";
import { canCreateProject } from "@/features/teams/team-permissions";

import { projectsApi } from "./projects.api";

export function ProjectsRoute() {
  const { activeTeam, activeRole } = useTeam();
  const teamId = activeTeam?.id;
  const showCreateProject = Boolean(activeRole && canCreateProject(activeRole));

  const projectsQuery = useQuery({
    queryKey: ["projects", teamId],
    queryFn: () => projectsApi.list(teamId as string),
    enabled: Boolean(teamId),
  });

  if (!teamId) {
    return <p className="text-sm text-muted-foreground">Select a team to view its projects.</p>;
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold">Projects</h1>
          <p className="text-sm text-muted-foreground">
            Deployable projects owned by {activeTeam?.name}.
          </p>
        </div>
        {showCreateProject && (
          <Button asChild>
            <Link to="/projects/new">
              <PlusIcon /> New project
            </Link>
          </Button>
        )}
      </div>

      {projectsQuery.isLoading && <Skeleton className="h-64 w-full" aria-busy="true" />}
      {projectsQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {projectsQuery.error instanceof Error
            ? projectsQuery.error.message
            : "Unable to load projects."}
        </p>
      )}
      {projectsQuery.data &&
        (projectsQuery.data.projects.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <FolderGitIcon />
              </EmptyMedia>
              <EmptyTitle>No projects yet</EmptyTitle>
              <EmptyDescription>
                {showCreateProject
                  ? "Create your first project to start deploying static sites."
                  : "No projects are available for this team."}
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Runtime</TableHead>
                <TableHead>Repository</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Created</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {projectsQuery.data.projects.map((project) => (
                <TableRow key={project.id}>
                  <TableCell>
                    <Link to={`/projects/${project.id}`} className="font-medium hover:underline">
                      {project.name}
                    </Link>
                    <p className="text-xs text-muted-foreground">{project.slug}</p>
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline">{project.runtime}</Badge>
                  </TableCell>
                  <TableCell className="max-w-56 truncate text-sm text-muted-foreground">
                    {project.repository_url ?? "—"}
                  </TableCell>
                  <TableCell>
                    {project.archived_at ? (
                      <Badge variant="secondary">Archived</Badge>
                    ) : (
                      <Badge variant="success">Active</Badge>
                    )}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(project.created_at).toLocaleDateString()}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}
    </div>
  );
}
