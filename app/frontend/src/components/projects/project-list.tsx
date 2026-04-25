import type { Project } from "@/api/projects";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { ProjectStatusBadge } from "./project-status-badge";

type ProjectListProps = {
  isLoading: boolean;
  onViewProject: (projectId: string) => void;
  projects: Project[];
};

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function projectStatusDescription(project: Project) {
  if (project.status === "soft_deleted") {
    return "Soft deleted project";
  }

  if (project.status === "archived") {
    return "Archived project";
  }

  return "Active project";
}

function projectLastEvent(project: Project) {
  if (project.soft_deleted_at) {
    return { label: "Soft deleted at", value: project.soft_deleted_at };
  }

  if (project.archived_at) {
    return { label: "Archived", value: project.archived_at };
  }

  return { label: "Updated", value: project.updated_at };
}

export function ProjectList({ isLoading, onViewProject, projects }: ProjectListProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Project inventory</h2>
        </CardTitle>
        <CardDescription>Track active and archived projects for this workspace.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {isLoading ? (
          <div className="space-y-3">
            <Skeleton className="h-24" />
            <Skeleton className="h-24" />
          </div>
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
          projects.map((project) => {
            const lastEvent = projectLastEvent(project);

            return (
              <Card key={project.id}>
                <CardHeader>
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div className="space-y-1">
                      <CardDescription>{project.slug}</CardDescription>
                      <CardTitle>
                        <h3>{project.name}</h3>
                      </CardTitle>
                      <CardDescription>{projectStatusDescription(project)}</CardDescription>
                    </div>
                    <ProjectStatusBadge status={project.status} />
                  </div>
                </CardHeader>
                <CardContent className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-3">
                  <div className="space-y-1">
                    <p>Created</p>
                    <p className="font-medium text-foreground">
                      {formatTimestamp(project.created_at)}
                    </p>
                  </div>
                  <div className="space-y-1">
                    <p>{lastEvent.label}</p>
                    <p className="font-medium text-foreground">
                      {formatTimestamp(lastEvent.value)}
                    </p>
                  </div>
                  <div className="sm:col-span-3">
                    <Button
                      onClick={() => onViewProject(project.id)}
                      type="button"
                      variant="outline"
                    >
                      View details
                    </Button>
                  </div>
                </CardContent>
              </Card>
            );
          })
        )}
      </CardContent>
    </Card>
  );
}
