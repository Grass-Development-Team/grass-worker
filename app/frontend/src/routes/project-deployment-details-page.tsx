import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ApiError } from "@/api/client";
import {
  deploymentArtifactsQueryKey,
  deploymentBuildLogQueryKey,
  deploymentQueryKey,
  deploymentsQueryKey,
  getProjectDeployment,
  getProjectDeploymentArtifacts,
  getProjectDeploymentBuildLog,
  transitionProjectDeployment,
  uploadProjectDeploymentBundle,
  type Deployment,
  type DeploymentArtifact,
  type DeploymentStatus,
} from "@/api/deployments";
import {
  activateProjectRelease,
  getProjectRelease,
  projectReleaseQueryKey,
  releasePublicUrl,
} from "@/api/releases";
import { ConsolePageHeader } from "@/components/console/console-page-header";
import { DeploymentOverviewCard } from "@/components/deployments/deployment-overview-card";
import { DeploymentStatusBadge } from "@/components/deployments/deployment-status-badge";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }

  return fallback;
}

function formatTimestamp(value: string | null) {
  if (!value) return "Not set";

  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function availableTransitions(
  status: DeploymentStatus,
): Array<{
  status: Exclude<DeploymentStatus, "pending">;
  label: string;
  pendingLabel: string;
  variant?: "default" | "outline" | "destructive";
}> {
  if (status === "pending") {
    return [
      {
        status: "processing",
        label: "Start processing",
        pendingLabel: "Starting...",
      },
      {
        status: "canceled",
        label: "Cancel deployment",
        pendingLabel: "Canceling...",
        variant: "outline",
      },
    ];
  }

  if (status === "processing") {
    return [
      {
        status: "ready",
        label: "Mark ready",
        pendingLabel: "Marking ready...",
      },
      {
        status: "failed",
        label: "Mark failed",
        pendingLabel: "Marking failed...",
        variant: "destructive",
      },
      {
        status: "canceled",
        label: "Cancel deployment",
        pendingLabel: "Canceling...",
        variant: "outline",
      },
    ];
  }

  return [];
}

function updateDeploymentCollection(
  deployments: Deployment[] | undefined,
  nextDeployment: Deployment,
) {
  return deployments?.map((deployment) =>
    deployment.id === nextDeployment.id ? nextDeployment : deployment,
  );
}

function RegisterArtifactCard({
  disabled,
  disabledReason,
  error,
  isSubmitting,
  onRegister,
  onResetError,
  resetToken,
}: {
  disabled: boolean;
  disabledReason: string | null;
  error: string | null;
  isSubmitting: boolean;
  onRegister: (bundle: File) => void;
  onResetError: () => void;
  resetToken: number;
}) {
  const [bundle, setBundle] = React.useState<File | null>(null);

  React.useEffect(() => {
    setBundle(null);
  }, [resetToken]);

  const submitBundle = () => {
    if (!bundle) {
      return;
    }

    onRegister(bundle);
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Upload static site bundle</h2>
        </CardTitle>
        <CardDescription>
          Upload a `.zip` archive with a root `index.html`. The server will extract it, register
          the `static_site` artifact, and move this deployment to `ready`.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            submitBundle();
          }}
        >
          <div className="space-y-2">
            <Label htmlFor="deployment-artifact-bundle">Bundle file</Label>
            <Input
              accept=".zip,application/zip"
              disabled={disabled || isSubmitting}
              id="deployment-artifact-bundle"
              onChange={(event) => {
                setBundle(event.target.files?.[0] ?? null);
                onResetError();
              }}
              required
              type="file"
            />
            <p className="text-sm text-muted-foreground">
              Include `index.html` at the archive root plus any nested assets your site needs.
            </p>
          </div>

          {bundle ? (
            <div className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-2">
              <div className="space-y-1">
                <p>Selected file</p>
                <p className="font-medium text-foreground">{bundle.name}</p>
              </div>
              <div className="space-y-1">
                <p>Size bytes</p>
                <p className="font-medium text-foreground">{bundle.size}</p>
              </div>
            </div>
          ) : null}

          {disabledReason ? (
            <Alert>
              <AlertTitle>Bundle upload unavailable</AlertTitle>
              <AlertDescription>{disabledReason}</AlertDescription>
            </Alert>
          ) : null}

          {error ? (
            <Alert variant="destructive">
              <AlertTitle>Bundle upload failed</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}

          <Button
            className="w-full"
            disabled={disabled || isSubmitting || !bundle}
            onClick={submitBundle}
            type="button"
          >
            {isSubmitting ? "Uploading bundle..." : "Upload bundle"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}

export function ProjectDeploymentDetailsPage() {
  const navigate = useNavigate();
  const { projectId, deploymentId } = useParams<{
    projectId: string;
    deploymentId: string;
  }>();
  const queryClient = useQueryClient();
  const [artifactResetToken, setArtifactResetToken] = React.useState(0);
  const query = useQuery({
    queryKey: deploymentQueryKey(projectId ?? "", deploymentId ?? ""),
    queryFn: () => getProjectDeployment(projectId ?? "", deploymentId ?? ""),
    enabled: Boolean(projectId && deploymentId),
  });
  const artifactsQuery = useQuery({
    queryKey: deploymentArtifactsQueryKey(projectId ?? "", deploymentId ?? ""),
    queryFn: () => getProjectDeploymentArtifacts(projectId ?? "", deploymentId ?? ""),
    enabled: Boolean(projectId && deploymentId),
  });
  const hasBuildLogArtifact = (artifactsQuery.data ?? []).some(
    (artifact) => artifact.kind === "build_log",
  );
  const buildLogQuery = useQuery({
    queryKey: deploymentBuildLogQueryKey(projectId ?? "", deploymentId ?? ""),
    queryFn: () => getProjectDeploymentBuildLog(projectId ?? "", deploymentId ?? ""),
    enabled: Boolean(projectId && deploymentId && hasBuildLogArtifact),
    retry: false,
  });
  const releaseQuery = useQuery({
    queryKey: projectReleaseQueryKey(projectId ?? ""),
    queryFn: () => getProjectRelease(projectId ?? ""),
    enabled: Boolean(projectId),
    retry: false,
  });
  const transitionMutation = useMutation({
    mutationFn: (status: Exclude<DeploymentStatus, "pending">) =>
      transitionProjectDeployment(projectId ?? "", deploymentId ?? "", { status }),
    onSuccess: async (deployment) => {
      if (!projectId || !deploymentId) return;

      queryClient.setQueryData(deploymentQueryKey(projectId, deploymentId), deployment);
      queryClient.setQueryData<Deployment[]>(
        deploymentsQueryKey(projectId),
        (current) => updateDeploymentCollection(current, deployment),
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: deploymentQueryKey(projectId, deploymentId) }),
        queryClient.invalidateQueries({ queryKey: deploymentsQueryKey(projectId) }),
      ]);
    },
  });
  const registerArtifactMutation = useMutation({
    mutationFn: (bundle: File) =>
      uploadProjectDeploymentBundle(projectId ?? "", deploymentId ?? "", bundle),
    onSuccess: async ({ deployment, artifact }) => {
      if (!projectId || !deploymentId) return;

      queryClient.setQueryData(deploymentQueryKey(projectId, deploymentId), deployment);
      queryClient.setQueryData<Deployment[]>(
        deploymentsQueryKey(projectId),
        (current) => updateDeploymentCollection(current, deployment),
      );
      queryClient.setQueryData<DeploymentArtifact[]>(
        deploymentArtifactsQueryKey(projectId, deploymentId),
        (current) => [
          artifact,
          ...(current ?? []).filter((item) => item.id !== artifact.id),
        ],
      );
      setArtifactResetToken((current) => current + 1);
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: deploymentQueryKey(projectId, deploymentId),
        }),
        queryClient.invalidateQueries({
          queryKey: deploymentsQueryKey(projectId),
        }),
        queryClient.invalidateQueries({
          queryKey: deploymentArtifactsQueryKey(projectId, deploymentId),
        }),
      ]);
    },
  });
  const activateReleaseMutation = useMutation({
    mutationFn: () => activateProjectRelease(projectId ?? "", deploymentId ?? ""),
    onSuccess: async (release) => {
      if (!projectId) return;

      queryClient.setQueryData(projectReleaseQueryKey(projectId), release);
      await queryClient.invalidateQueries({ queryKey: projectReleaseQueryKey(projectId) });
    },
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

  const deployment = query.data;
  const transitionActions = availableTransitions(deployment.status);
  const transitionError = transitionMutation.isError
    ? errorMessage(transitionMutation.error, "Unable to update deployment status")
    : null;
  const registerArtifactError = registerArtifactMutation.isError
    ? errorMessage(registerArtifactMutation.error, "Unable to upload bundle")
    : null;
  const artifactRegistrationDisabled =
    deployment.status === "ready" ||
    deployment.status === "failed" ||
    deployment.status === "canceled";
  const artifactRegistrationDisabledReason =
    deployment.status === "ready"
      ? "This deployment already has a completed static site upload."
      : deployment.status === "failed" || deployment.status === "canceled"
        ? "Create a new deployment before uploading another bundle."
        : null;
  const isRefreshing = query.isFetching || artifactsQuery.isFetching || buildLogQuery.isFetching;
  const hasStaticSiteArtifact = (artifactsQuery.data ?? []).some(
    (artifact) => artifact.kind === "static_site",
  );
  const isCurrentlyLive = releaseQuery.data?.active_deployment_id === deployment.id;
  const liveSiteUrl = releasePublicUrl(releaseQuery.data?.primary_host ?? null);
  const activateReleaseDisabled =
    deployment.status !== "ready" || !hasStaticSiteArtifact || activateReleaseMutation.isPending;
  const activateReleaseDisabledReason =
    deployment.status !== "ready"
      ? "Only ready deployments can be activated."
      : !hasStaticSiteArtifact
        ? "Upload a static site bundle before activating the release."
        : null;

  return (
    <div className="space-y-6">
      <ConsolePageHeader
        actions={
          <>
            <Button
              onClick={() => void navigate(`/projects/${projectId}`)}
              type="button"
              variant="outline"
            >
              Back to project
            </Button>
            <Button
              disabled={isRefreshing}
              onClick={() => {
                void Promise.all([query.refetch(), artifactsQuery.refetch(), buildLogQuery.refetch()]);
              }}
              type="button"
              variant="outline"
            >
              {isRefreshing ? "Refreshing..." : "Refresh details"}
            </Button>
          </>
        }
        description="Deployment record, lifecycle state, and attached artifact inventory."
        eyebrow="Deployment"
        title="Deployment details"
      />

      <DeploymentOverviewCard deployment={deployment} />

      {deployment.status === "failed" ? (
        <Alert variant="destructive">
          <AlertTitle>Deployment failed</AlertTitle>
          <AlertDescription>
            {deployment.failure_message ?? "The node worker reported a deployment failure."}
          </AlertDescription>
        </Alert>
      ) : null}

      <Card>
        <CardHeader>
          <CardTitle>
            <h2>Release controls</h2>
          </CardTitle>
          <CardDescription>
            Promote this deployment to the live site once the static artifact is ready.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {isCurrentlyLive ? (
            <Alert>
              <AlertTitle>Currently live</AlertTitle>
              <AlertDescription>
                {liveSiteUrl
                  ? `This deployment is serving traffic at ${liveSiteUrl}.`
                  : "This deployment is live, but the project does not have a primary host yet."}
              </AlertDescription>
            </Alert>
          ) : null}

          {activateReleaseMutation.isError ? (
            <Alert variant="destructive">
              <AlertTitle>Release activation failed</AlertTitle>
              <AlertDescription>
                {errorMessage(
                  activateReleaseMutation.error,
                  "Unable to activate this deployment as the live release.",
                )}
              </AlertDescription>
            </Alert>
          ) : null}

          {activateReleaseDisabledReason ? (
            <Alert>
              <AlertTitle>Release activation unavailable</AlertTitle>
              <AlertDescription>{activateReleaseDisabledReason}</AlertDescription>
            </Alert>
          ) : null}

          <div className="flex flex-wrap gap-3">
            <Button
              disabled={activateReleaseDisabled || isCurrentlyLive}
              onClick={() => activateReleaseMutation.mutate()}
              type="button"
            >
              {activateReleaseMutation.isPending ? "Activating..." : "Activate release"}
            </Button>
            {liveSiteUrl ? (
              <Button asChild type="button" variant="outline">
                <a href={liveSiteUrl} rel="noreferrer" target="_blank">
                  Open live site
                </a>
              </Button>
            ) : (
              <Button disabled type="button" variant="outline">
                Open live site
              </Button>
            )}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div className="space-y-1">
              <CardTitle>
                <h2>Lifecycle controls</h2>
              </CardTitle>
              <CardDescription>
                Advance this deployment through the allowed Phase 3 status transitions.
              </CardDescription>
            </div>
            <DeploymentStatusBadge status={deployment.status} />
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {transitionError ? (
            <Alert variant="destructive">
              <AlertTitle>Deployment status update failed</AlertTitle>
              <AlertDescription>{transitionError}</AlertDescription>
            </Alert>
          ) : null}

          <div className="grid gap-3 sm:grid-cols-3">
            {transitionActions.length > 0 ? (
              transitionActions.map((action) => (
                <Button
                  disabled={transitionMutation.isPending}
                  key={action.status}
                  onClick={() => transitionMutation.mutate(action.status)}
                  type="button"
                  variant={action.variant ?? "default"}
                >
                  {transitionMutation.isPending &&
                  transitionMutation.variables === action.status
                    ? action.pendingLabel
                    : action.label}
                </Button>
              ))
            ) : (
              <Card className="sm:col-span-3">
                <CardContent className="pt-6 text-sm text-muted-foreground">
                  This deployment is in a terminal state. No further Phase 3 transitions are
                  available.
                </CardContent>
              </Card>
            )}
          </div>
        </CardContent>
      </Card>

      {artifactsQuery.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Unable to load deployment artifacts</AlertTitle>
          <AlertDescription>
            {errorMessage(artifactsQuery.error, "Deployment artifact lookup failed.")}
          </AlertDescription>
        </Alert>
      ) : null}

      <Card>
        <CardHeader>
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div className="space-y-1">
              <CardTitle>
                <h2>Build log</h2>
              </CardTitle>
              <CardDescription>
                Raw node-worker output for clone, install, build, and upload steps.
              </CardDescription>
            </div>
            {deployment.last_stage ? <Badge variant="outline">{deployment.last_stage}</Badge> : null}
          </div>
        </CardHeader>
        <CardContent>
          {!hasBuildLogArtifact ? (
            <Alert>
              <AlertTitle>No build log uploaded</AlertTitle>
              <AlertDescription>
                The node worker has not attached build output to this deployment yet.
              </AlertDescription>
            </Alert>
          ) : buildLogQuery.isPending ? (
            <Skeleton className="h-40" />
          ) : buildLogQuery.isError ? (
            <Alert>
              <AlertTitle>Build log unavailable</AlertTitle>
              <AlertDescription>
                {errorMessage(buildLogQuery.error, "No build log has been uploaded yet.")}
              </AlertDescription>
            </Alert>
          ) : buildLogQuery.data.trim() === "" ? (
            <Alert>
              <AlertTitle>Build log is empty</AlertTitle>
              <AlertDescription>
                The node worker uploaded an empty build log for this deployment.
              </AlertDescription>
            </Alert>
          ) : (
            <pre className="max-h-[32rem] overflow-auto rounded-lg border bg-muted/60 p-4 text-xs leading-relaxed text-foreground">
              <code>{buildLogQuery.data}</code>
            </pre>
          )}
        </CardContent>
      </Card>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,2fr)_minmax(320px,1fr)]">
        <Card>
          <CardHeader>
            <CardTitle>
              <h2>Artifacts</h2>
            </CardTitle>
            <CardDescription>
              Registered deployment outputs and logs attached to this deployment record.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {artifactsQuery.isPending ? (
              <div className="space-y-3">
                <Skeleton className="h-24" />
                <Skeleton className="h-24" />
              </div>
            ) : (artifactsQuery.data ?? []).length === 0 ? (
              <Card>
                <CardHeader>
                  <CardTitle>No artifacts registered yet</CardTitle>
                  <CardDescription>
                    This deployment record does not have any artifact metadata attached yet.
                  </CardDescription>
                </CardHeader>
              </Card>
            ) : (
              (artifactsQuery.data ?? []).map((artifact) => (
                <Card key={artifact.id}>
                  <CardHeader>
                    <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                      <div className="space-y-1">
                        <CardDescription>{artifact.id}</CardDescription>
                        <CardTitle>
                          <h3>{artifact.storage_path}</h3>
                        </CardTitle>
                        <CardDescription>
                          Registered {formatTimestamp(artifact.created_at)}
                        </CardDescription>
                      </div>
                      <Badge variant="outline">{artifact.kind}</Badge>
                    </div>
                  </CardHeader>
                  <CardContent className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-2">
                    <div className="space-y-1">
                      <p>SHA256 checksum</p>
                      <p className="font-medium text-foreground">
                        {artifact.checksum_sha256 ?? "Not set"}
                      </p>
                    </div>
                    <div className="space-y-1">
                      <p>Size bytes</p>
                      <p className="font-medium text-foreground">
                        {artifact.size_bytes ?? "Not set"}
                      </p>
                    </div>
                  </CardContent>
                </Card>
              ))
            )}
          </CardContent>
        </Card>

        <RegisterArtifactCard
          disabled={artifactRegistrationDisabled}
          disabledReason={artifactRegistrationDisabledReason}
          error={registerArtifactError}
          isSubmitting={registerArtifactMutation.isPending}
          onRegister={(input) => registerArtifactMutation.mutate(input)}
          onResetError={() => registerArtifactMutation.reset()}
          resetToken={artifactResetToken}
        />
      </div>
    </div>
  );
}
