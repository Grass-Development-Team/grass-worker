import type { DeploymentStatus } from "@/api/deployments";
import { Badge } from "@/components/ui/badge";

const statusLabels: Record<DeploymentStatus, string> = {
  pending: "Pending",
  processing: "Processing",
  ready: "Ready",
  failed: "Failed",
  canceled: "Canceled",
};

const statusVariants: Record<
  DeploymentStatus,
  "default" | "secondary" | "destructive" | "outline"
> = {
  pending: "secondary",
  processing: "outline",
  ready: "default",
  failed: "destructive",
  canceled: "outline",
};

export function deploymentStatusLabel(status: DeploymentStatus) {
  return statusLabels[status];
}

export function DeploymentStatusBadge({ status }: { status: DeploymentStatus }) {
  return <Badge variant={statusVariants[status]}>{statusLabels[status]}</Badge>;
}
