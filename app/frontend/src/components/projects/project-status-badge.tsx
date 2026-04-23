import { Badge } from "@/components/ui/badge";
import type { ProjectStatus } from "@/api/projects";

const statusLabels: Record<ProjectStatus, string> = {
  active: "Active",
  archived: "Archived",
  soft_deleted: "Soft deleted",
};

const statusVariants: Record<ProjectStatus, "default" | "secondary" | "destructive"> = {
  active: "default",
  archived: "secondary",
  soft_deleted: "destructive",
};

export function projectStatusLabel(status: ProjectStatus) {
  return statusLabels[status];
}

export function ProjectStatusBadge({ status }: { status: ProjectStatus }) {
  return <Badge variant={statusVariants[status]}>{statusLabels[status]}</Badge>;
}
