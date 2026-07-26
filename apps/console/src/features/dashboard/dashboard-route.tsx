import { useQuery } from "@tanstack/react-query";
import { FolderGitIcon, PlusIcon, SettingsIcon, ShieldCheckIcon, UsersIcon } from "lucide-react";
import { Link } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { useAuth } from "@/features/auth/auth-context";
import { projectsApi } from "@/features/projects/projects.api";
import { useTeam } from "@/features/teams/team-context";

const initial = (value: string) => value.slice(0, 1).toUpperCase();

export function DashboardRoute() {
  const { user } = useAuth();
  const { activeTeam } = useTeam();
  const teamId = activeTeam?.id;

  const projectsQuery = useQuery({
    queryKey: ["projects", teamId],
    queryFn: () => projectsApi.list(teamId as string),
    enabled: Boolean(teamId),
  });
  const projects = projectsQuery.data?.projects ?? [];

  return (
    <div className="flex w-full flex-col gap-8">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">
            {activeTeam?.name ?? "Workspace"}
          </h1>
          <p className="text-sm text-muted-foreground">Projects and controls for this workspace.</p>
        </div>
        <Button asChild>
          <Link to="/projects">
            <PlusIcon /> New Project
          </Link>
        </Button>
      </div>

      <section className="space-y-3">
        <h2 className="text-base font-semibold">Projects</h2>
        {projectsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
        {projectsQuery.isError && (
          <p role="alert" className="text-sm text-destructive">
            {projectsQuery.error instanceof Error
              ? projectsQuery.error.message
              : "Unable to load projects."}
          </p>
        )}
        {projectsQuery.data &&
          (projects.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <FolderGitIcon />
                </EmptyMedia>
                <EmptyTitle>No projects yet</EmptyTitle>
                <EmptyDescription>Create your first project to start deploying.</EmptyDescription>
              </EmptyHeader>
              <Button asChild variant="outline" size="sm">
                <Link to="/projects">Create a project</Link>
              </Button>
            </Empty>
          ) : (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
              {projects.map((project) => (
                <Link key={project.id} to={`/projects/${project.id}`} className="group">
                  <Card className="h-full gap-0 py-5 transition-colors group-hover:border-foreground/25">
                    <CardContent className="flex items-start gap-3 px-5">
                      <div className="grid size-9 shrink-0 place-items-center rounded-full border bg-muted/60 text-sm font-semibold">
                        {initial(project.name)}
                      </div>
                      <div className="min-w-0 flex-1 space-y-1">
                        <div className="flex items-center justify-between gap-2">
                          <p className="truncate text-sm font-medium">{project.name}</p>
                          {project.archived_at ? (
                            <Badge variant="secondary">Archived</Badge>
                          ) : (
                            <Badge variant="outline">{project.runtime}</Badge>
                          )}
                        </div>
                        <p className="truncate text-xs text-muted-foreground">{project.slug}</p>
                        <p className="truncate text-xs text-muted-foreground">
                          {project.repository_url ?? "No repository"}
                        </p>
                        <p className="text-xs text-muted-foreground">
                          Updated {new Date(project.updated_at).toLocaleDateString()}
                        </p>
                      </div>
                    </CardContent>
                  </Card>
                </Link>
              ))}
            </div>
          ))}
      </section>

      <section className="space-y-3">
        <h2 className="text-base font-semibold">Workspace controls</h2>
        <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
          <Button asChild variant="outline" className="h-11 justify-start">
            <Link to="/settings/team">
              <SettingsIcon />
              Team settings
            </Link>
          </Button>
          <Button asChild variant="outline" className="h-11 justify-start">
            <Link to="/settings/members">
              <UsersIcon />
              Members
            </Link>
          </Button>
          {user?.platform_role === "admin" && (
            <Button asChild variant="outline" className="h-11 justify-start">
              <Link to="/admin">
                <ShieldCheckIcon />
                Administration
              </Link>
            </Button>
          )}
        </div>
      </section>
    </div>
  );
}
