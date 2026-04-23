import type { Project } from "@/api/projects";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { ProjectStatusBadge, projectStatusLabel } from "./project-status-badge";

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function latestProjectEvent(project: Project) {
  if (project.soft_deleted_at) {
    return { label: "Soft deleted", value: project.soft_deleted_at };
  }

  if (project.archived_at) {
    return { label: "Archived", value: project.archived_at };
  }

  return { label: "Updated", value: project.updated_at };
}

export function ProjectOverviewCard({ project }: { project: Project }) {
  const latestEvent = latestProjectEvent(project);

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div className="space-y-1">
            <CardTitle>
              <h2>Overview</h2>
            </CardTitle>
            <CardDescription>Core metadata for the selected project.</CardDescription>
          </div>
          <ProjectStatusBadge status={project.status} />
        </div>
      </CardHeader>
      <CardContent className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-3">
        <div className="space-y-1">
          <p>Slug</p>
          <p className="font-medium text-foreground">{project.slug}</p>
        </div>
        <div className="space-y-1">
          <p>Status</p>
          <p className="font-medium text-foreground">{projectStatusLabel(project.status)}</p>
        </div>
        <div className="space-y-1">
          <p>Created</p>
          <p className="font-medium text-foreground">{formatTimestamp(project.created_at)}</p>
        </div>
        <div className="space-y-1 sm:col-span-3">
          <p>{latestEvent.label}</p>
          <p className="font-medium text-foreground">{formatTimestamp(latestEvent.value)}</p>
        </div>
      </CardContent>
    </Card>
  );
}
