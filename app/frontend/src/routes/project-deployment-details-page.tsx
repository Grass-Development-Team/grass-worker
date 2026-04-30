import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ApiError } from "@/api/client";
import {
  createProjectDeploymentArtifact,
  deploymentArtifactsQueryKey,
  deploymentQueryKey,
  deploymentsQueryKey,
  getProjectDeployment,
  getProjectDeploymentArtifacts,
  transitionProjectDeployment,
  type Deployment,
  type DeploymentArtifact,
  type DeploymentArtifactKind,
  type DeploymentStatus,
} from "@/api/deployments";
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

function normalizeOptionalInput(value: string) {
  const normalized = value.trim();
  return normalized === "" ? undefined : normalized;
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
  onRegister: (input: {
    kind: DeploymentArtifactKind;
    storage_path: string;
    checksum_sha256?: string;
    size_bytes?: number;
  }) => void;
  onResetError: () => void;
  resetToken: number;
}) {
  const [kind, setKind] = React.useState<DeploymentArtifactKind>("static_site");
  const [storagePath, setStoragePath] = React.useState("");
  const [checksum, setChecksum] = React.useState("");
  const [sizeBytes, setSizeBytes] = React.useState("");

  React.useEffect(() => {
    setKind("static_site");
    setStoragePath("");
    setChecksum("");
    setSizeBytes("");
  }, [resetToken]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Register artifact</h2>
        </CardTitle>
        <CardDescription>
          Attach artifact metadata to this deployment record without waiting for the later node
          pipeline phases.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();

            const normalizedStoragePath = normalizeOptionalInput(storagePath);
            if (!normalizedStoragePath) {
              return;
            }

            const normalizedSize = normalizeOptionalInput(sizeBytes);
            onRegister({
              kind,
              storage_path: normalizedStoragePath,
              checksum_sha256: normalizeOptionalInput(checksum),
              size_bytes: normalizedSize ? Number.parseInt(normalizedSize, 10) : undefined,
            });
          }}
        >
          <div className="space-y-2">
            <Label htmlFor="deployment-artifact-kind">Artifact kind</Label>
            <select
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-xs outline-none transition-[color,box-shadow] focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
              disabled={disabled || isSubmitting}
              id="deployment-artifact-kind"
              onChange={(event) => {
                setKind(event.target.value as DeploymentArtifactKind);
                onResetError();
              }}
              value={kind}
            >
              <option value="static_site">static_site</option>
              <option value="build_log">build_log</option>
            </select>
          </div>

          <div className="space-y-2">
            <Label htmlFor="deployment-artifact-storage-path">Storage path</Label>
            <Input
              disabled={disabled || isSubmitting}
              id="deployment-artifact-storage-path"
              onChange={(event) => {
                setStoragePath(event.target.value);
                onResetError();
              }}
              placeholder="s3://artifacts/docs-site"
              required
              value={storagePath}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="deployment-artifact-checksum">SHA256 checksum</Label>
            <Input
              disabled={disabled || isSubmitting}
              id="deployment-artifact-checksum"
              onChange={(event) => {
                setChecksum(event.target.value);
                onResetError();
              }}
              placeholder="abc123"
              value={checksum}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="deployment-artifact-size-bytes">Size bytes</Label>
            <Input
              disabled={disabled || isSubmitting}
              id="deployment-artifact-size-bytes"
              inputMode="numeric"
              onChange={(event) => {
                setSizeBytes(event.target.value);
                onResetError();
              }}
              placeholder="1024"
              value={sizeBytes}
            />
          </div>

          {disabledReason ? (
            <Alert>
              <AlertTitle>Artifact registration unavailable</AlertTitle>
              <AlertDescription>{disabledReason}</AlertDescription>
            </Alert>
          ) : null}

          {error ? (
            <Alert variant="destructive">
              <AlertTitle>Artifact registration failed</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}

          <Button className="w-full" disabled={disabled || isSubmitting} type="submit">
            {isSubmitting ? "Registering artifact..." : "Register artifact"}
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
    mutationFn: (input: {
      kind: DeploymentArtifactKind;
      storage_path: string;
      checksum_sha256?: string;
      size_bytes?: number;
    }) => createProjectDeploymentArtifact(projectId ?? "", deploymentId ?? "", input),
    onSuccess: async (artifact) => {
      if (!projectId || !deploymentId) return;

      queryClient.setQueryData<DeploymentArtifact[]>(
        deploymentArtifactsQueryKey(projectId, deploymentId),
        (current) => [
          artifact,
          ...(current ?? []).filter((item) => item.id !== artifact.id),
        ],
      );
      setArtifactResetToken((current) => current + 1);
      await queryClient.invalidateQueries({
        queryKey: deploymentArtifactsQueryKey(projectId, deploymentId),
      });
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
    ? errorMessage(registerArtifactMutation.error, "Unable to register artifact")
    : null;
  const artifactRegistrationDisabled = deployment.status === "pending";
  const artifactRegistrationDisabledReason = artifactRegistrationDisabled
    ? "Start processing before registering artifacts."
    : null;
  const isRefreshing = query.isFetching || artifactsQuery.isFetching;

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
                void Promise.all([query.refetch(), artifactsQuery.refetch()]);
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
