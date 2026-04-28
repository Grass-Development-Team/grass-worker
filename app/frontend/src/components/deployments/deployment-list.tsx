import type { Deployment } from "@/api/deployments";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { DeploymentStatusBadge } from "./deployment-status-badge";

function formatTimestamp(value: string | null) {
  if (!value) return "Not set";

  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

export function DeploymentList({
  deployments,
  isLoading,
  onViewDeployment,
}: {
  deployments: Deployment[];
  isLoading: boolean;
  onViewDeployment: (deploymentId: string) => void;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Deployment history</h2>
        </CardTitle>
        <CardDescription>Track deployment records for this project.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {isLoading ? (
          <div className="space-y-3">
            <Skeleton className="h-24" />
            <Skeleton className="h-24" />
          </div>
        ) : deployments.length === 0 ? (
          <Card>
            <CardHeader>
              <CardTitle>No deployments yet</CardTitle>
              <CardDescription>
                Create the first deployment record to establish project release history.
              </CardDescription>
            </CardHeader>
          </Card>
        ) : (
          deployments.map((deployment) => (
            <Card key={deployment.id}>
              <CardHeader>
                <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div className="space-y-1">
                    <CardDescription>{deployment.id}</CardDescription>
                    <CardTitle>
                      <h3>{deployment.source_branch ?? "Manual deployment"}</h3>
                    </CardTitle>
                    <CardDescription>Source revision</CardDescription>
                    <p className="text-sm font-medium text-foreground">
                      {deployment.source_revision ?? "not set"}
                    </p>
                  </div>
                  <DeploymentStatusBadge status={deployment.status} />
                </div>
              </CardHeader>
              <CardContent className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-3">
                <div className="space-y-1">
                  <p>Created</p>
                  <p className="font-medium text-foreground">
                    {formatTimestamp(deployment.created_at)}
                  </p>
                </div>
                <div className="space-y-1">
                  <p>Started</p>
                  <p className="font-medium text-foreground">
                    {formatTimestamp(deployment.started_at)}
                  </p>
                </div>
                <div className="space-y-1">
                  <p>Finished</p>
                  <p className="font-medium text-foreground">
                    {formatTimestamp(deployment.finished_at)}
                  </p>
                </div>
                <div className="sm:col-span-3">
                  <Button
                    onClick={() => onViewDeployment(deployment.id)}
                    type="button"
                    variant="outline"
                  >
                    View deployment details
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))
        )}
      </CardContent>
    </Card>
  );
}
