import type { Deployment } from "@/api/deployments";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { DeploymentStatusBadge } from "./deployment-status-badge";

function formatTimestamp(value: string | null) {
  if (!value) return "Not started";

  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

export function DeploymentOverviewCard({ deployment }: { deployment: Deployment }) {
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div className="space-y-1">
            <CardDescription>{deployment.id}</CardDescription>
            <CardTitle>Deployment details</CardTitle>
            <CardDescription>
              Deployment record and current lifecycle state for this project.
            </CardDescription>
          </div>
          <DeploymentStatusBadge status={deployment.status} />
        </div>
      </CardHeader>
      <CardContent className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-2">
        <div className="space-y-1">
          <p>Source branch</p>
          <p className="font-medium text-foreground">{deployment.source_branch ?? "Not set"}</p>
        </div>
        <div className="space-y-1">
          <p>Source revision</p>
          <p className="font-medium text-foreground">{deployment.source_revision ?? "Not set"}</p>
        </div>
        <div className="space-y-1">
          <p>Created</p>
          <p className="font-medium text-foreground">{formatTimestamp(deployment.created_at)}</p>
        </div>
        <div className="space-y-1">
          <p>Started</p>
          <p className="font-medium text-foreground">{formatTimestamp(deployment.started_at)}</p>
        </div>
        <div className="space-y-1">
          <p>Finished</p>
          <p className="font-medium text-foreground">{formatTimestamp(deployment.finished_at)}</p>
        </div>
        <div className="space-y-1">
          <p>Project id</p>
          <p className="font-medium text-foreground">{deployment.project_id}</p>
        </div>
      </CardContent>
    </Card>
  );
}
