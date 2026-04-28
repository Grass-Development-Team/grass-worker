import { useQuery } from "@tanstack/react-query";
import { useNavigate, useParams } from "react-router-dom";
import { ApiError } from "@/api/client";
import {
  deploymentQueryKey,
  getProjectDeployment,
} from "@/api/deployments";
import { ConsolePageHeader } from "@/components/console/console-page-header";
import { DeploymentOverviewCard } from "@/components/deployments/deployment-overview-card";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }

  return fallback;
}

export function ProjectDeploymentDetailsPage() {
  const navigate = useNavigate();
  const { projectId, deploymentId } = useParams<{
    projectId: string;
    deploymentId: string;
  }>();
  const query = useQuery({
    queryKey: deploymentQueryKey(projectId ?? "", deploymentId ?? ""),
    queryFn: () => getProjectDeployment(projectId ?? "", deploymentId ?? ""),
    enabled: Boolean(projectId && deploymentId),
  });

  if (!projectId || !deploymentId) {
    return (
      <Card className="w-full max-w-lg">
        <CardHeader>
          <CardTitle>
            <h1>Deployment unavailable</h1>
          </CardTitle>
          <CardDescription>The deployment route parameters are incomplete.</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  if (query.isPending) {
    return (
      <div className="space-y-6">
        <ConsolePageHeader eyebrow="Deployment" title="Loading deployment" />
        <Card>
          <CardHeader>
            <CardTitle>
              <h2>Loading deployment</h2>
            </CardTitle>
            <CardDescription>Fetching deployment metadata from the control API.</CardDescription>
          </CardHeader>
        </Card>
      </div>
    );
  }

  if (query.isError) {
    return (
      <div className="space-y-6">
        <ConsolePageHeader
          actions={
            <Button
              onClick={() => void navigate(`/projects/${projectId}`)}
              type="button"
              variant="outline"
            >
              Back to project
            </Button>
          }
          eyebrow="Deployment"
          title="Deployment unavailable"
        />
        <Alert variant="destructive">
          <AlertTitle>Unable to load deployment</AlertTitle>
          <AlertDescription>
            {errorMessage(query.error, "Deployment lookup failed.")}
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <ConsolePageHeader
        actions={
          <Button
            onClick={() => void navigate(`/projects/${projectId}`)}
            type="button"
            variant="outline"
          >
            Back to project
          </Button>
        }
        description="Read-only deployment record for this project."
        eyebrow="Deployment"
        title="Deployment details"
      />

      <DeploymentOverviewCard deployment={query.data} />

      <Card>
        <CardHeader>
          <CardTitle>
            <h2>Next steps</h2>
          </CardTitle>
          <CardDescription>
            Artifact browsing, logs, and deployment state transitions are not implemented in this
            iteration.
          </CardDescription>
        </CardHeader>
      </Card>
    </div>
  );
}
