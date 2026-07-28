import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BanIcon, ExternalLinkIcon, RotateCcwIcon, RocketIcon, UndoIcon } from "lucide-react";
import { useCallback, useState } from "react";
import { Link, useParams } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useTeam } from "@/features/teams/team-context";
import { canManageMembers } from "@/features/teams/team-permissions";

import {
  deploymentsApi,
  deploymentRefetchInterval,
  formatDuration,
  isBuildRunning,
  shortCommit,
  type DeploymentDetail,
} from "./deployments.api";
import { LogViewer } from "./components/log-viewer";
import { BuildStatusBadge, ReleaseStatusBadge, ServeStatusBadge } from "./components/status-badges";

export function DeploymentDetailRoute() {
  const { projectId, deploymentId } = useParams<{ projectId: string; deploymentId: string }>();
  const { activeRole } = useTeam();
  const queryClient = useQueryClient();
  const [actionError, setActionError] = useState<string | null>(null);

  const detailQuery = useQuery({
    queryKey: ["deployment", projectId, deploymentId],
    queryFn: () => deploymentsApi.detail(projectId as string, deploymentId as string),
    enabled: Boolean(projectId && deploymentId),
    refetchInterval: (query) => deploymentRefetchInterval(query.state.data?.deployment),
  });

  const invalidate = useCallback(
    () => queryClient.invalidateQueries({ queryKey: ["deployment", projectId, deploymentId] }),
    [queryClient, projectId, deploymentId],
  );

  const act = (action: () => Promise<unknown>) =>
    action()
      .then(() => {
        setActionError(null);
        return invalidate();
      })
      .catch((cause) =>
        setActionError(cause instanceof Error ? cause.message : "The action failed."),
      );

  const cancelMutation = useMutation({
    mutationFn: () => deploymentsApi.cancel(projectId as string, deploymentId as string),
  });
  const retryMutation = useMutation({
    mutationFn: () => deploymentsApi.retry(projectId as string, deploymentId as string),
  });
  const promoteMutation = useMutation({
    mutationFn: () => deploymentsApi.promote(projectId as string, deploymentId as string),
  });
  const rollbackMutation = useMutation({
    mutationFn: () => deploymentsApi.rollback(projectId as string, deploymentId as string),
  });

  if (detailQuery.isLoading) {
    return <Skeleton className="h-96 w-full" aria-busy="true" />;
  }
  if (detailQuery.isError || !detailQuery.data) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {detailQuery.error instanceof Error
          ? detailQuery.error.message
          : "Unable to load this deployment."}
      </p>
    );
  }

  const detail = detailQuery.data;
  const { deployment } = detail;
  const running = isBuildRunning(deployment.build_status);
  const isAdmin = activeRole ? canManageMembers(activeRole) : false;
  const url = deployment.production_url ?? deployment.preview_url;
  const buildReady = deployment.build_status === "ready";
  const lifecycleReady = buildReady && deployment.serve_status === "ready";
  const canQueueRelease = buildReady && deployment.serve_status === "retired";

  const showPromote =
    buildReady &&
    deployment.release_status !== "active" &&
    (!detail.review_required || deployment.release_status === "approved");
  const canPromote =
    showPromote && (lifecycleReady || canQueueRelease) && !deployment.release_pending;
  const canRetry = ["failed", "canceled"].includes(deployment.build_status);
  const showRollback = buildReady && deployment.release_status === "approved" && detail.was_active;
  const canRollback =
    showRollback && (lifecycleReady || canQueueRelease) && !deployment.release_pending;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="font-mono text-lg font-semibold">{deployment.id.slice(0, 8)}</h1>
            <Badge variant="outline" className="capitalize">
              {deployment.environment}
            </Badge>
            <BuildStatusBadge status={deployment.build_status} />
            <ServeStatusBadge status={deployment.serve_status} />
            <ReleaseStatusBadge status={deployment.release_status} />
            {deployment.overcommitted && <Badge variant="warning">Overflow placement</Badge>}
          </div>
          <p className="text-sm text-muted-foreground">
            Created {new Date(deployment.created_at).toLocaleString()}
            {deployment.triggered_by
              ? ` by ${deployment.triggered_by.display_name ?? deployment.triggered_by.email}`
              : ""}
          </p>
        </div>
        <Button variant="outline" asChild>
          <Link to={`/projects/${projectId}`}>Back to project</Link>
        </Button>
      </div>

      {actionError && (
        <p role="alert" className="text-sm text-destructive">
          {actionError}
        </p>
      )}

      {deployment.release_pending && (
        <p role="status" className="text-sm text-muted-foreground">
          Release queued. The current production version remains online until Serve is ready.
        </p>
      )}

      <div className="flex flex-wrap gap-2">
        {running && (
          <Button
            variant="destructive"
            onClick={() => act(cancelMutation.mutateAsync)}
            disabled={cancelMutation.isPending}
          >
            <BanIcon /> Cancel build
          </Button>
        )}
        {canRetry && (
          <Button
            variant="outline"
            onClick={() => act(retryMutation.mutateAsync)}
            disabled={retryMutation.isPending}
          >
            <RotateCcwIcon /> Retry
          </Button>
        )}
        {showPromote && isAdmin && (
          <Button
            onClick={() => act(promoteMutation.mutateAsync)}
            disabled={!canPromote || promoteMutation.isPending}
          >
            <RocketIcon /> Promote
          </Button>
        )}
        {showRollback && isAdmin && (
          <Button
            variant="outline"
            onClick={() => act(rollbackMutation.mutateAsync)}
            disabled={!canRollback || rollbackMutation.isPending}
          >
            <UndoIcon /> Roll back to this deployment
          </Button>
        )}
      </div>

      <div className="grid gap-6 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle>Build log</CardTitle>
          </CardHeader>
          <CardContent>
            <LogViewer
              projectId={projectId as string}
              deploymentId={deploymentId as string}
              buildStatus={deployment.build_status}
              onDone={invalidate}
            />
          </CardContent>
        </Card>

        <div className="space-y-6">
          <Card>
            <CardHeader>
              <CardTitle>Overview</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2 text-sm">
              <OverviewRow label="Runtime" value={deployment.runtime_kind} />
              <OverviewRow label="Branch" value={deployment.source.branch ?? "—"} />
              <OverviewRow label="Commit" value={shortCommit(deployment.source.commit_hash)} mono />
              {deployment.source.commit_message && (
                <OverviewRow label="Message" value={deployment.source.commit_message} />
              )}
              <OverviewRow
                label="Repository"
                value={deployment.source.repository_url ?? "—"}
                truncate
              />
              <OverviewRow label="Duration" value={formatDuration(deployment.duration_seconds)} />
              <OverviewRow label="Stage" value={deployment.build_stage ?? "—"} />
              <OverviewRow label="Build node" value={deployment.build_node?.name ?? "Unassigned"} />
              <OverviewRow label="Serve node" value={deployment.serve_node?.name ?? "Unassigned"} />
              <OverviewRow
                label="Serve resources"
                value={`${deployment.serve_resources.cpu_millicores}m · ${deployment.serve_resources.memory_mb} MB · ${deployment.serve_resources.disk_mb} MB disk`}
              />
              {url && (
                <div className="flex justify-between gap-2">
                  <span className="text-muted-foreground">URL</span>
                  <a
                    href={url}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1 truncate text-primary hover:underline"
                  >
                    <span className="truncate">{url}</span>
                    <ExternalLinkIcon className="size-3 shrink-0" />
                  </a>
                </div>
              )}
              {deployment.failure_message && (
                <div className="rounded-md border border-destructive/40 bg-destructive/5 p-2">
                  <p className="font-medium text-destructive">
                    {deployment.failure_code ?? "failed"}
                  </p>
                  <p className="text-destructive">{deployment.failure_message}</p>
                </div>
              )}
              {deployment.serve_failure_message && (
                <div className="rounded-md border border-destructive/40 bg-destructive/5 p-2">
                  <p className="font-medium text-destructive">
                    {deployment.serve_failure_code ?? "serve_failed"}
                  </p>
                  <p className="text-destructive">{deployment.serve_failure_message}</p>
                </div>
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Artifacts</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2 text-sm">
              {detail.artifacts.length === 0 ? (
                <p className="text-muted-foreground">No artifacts yet.</p>
              ) : (
                detail.artifacts.map((artifact) => (
                  <div key={artifact.id} className="rounded-md border p-2">
                    <p className="font-medium capitalize">{artifact.kind.replace("_", " ")}</p>
                    <p className="text-xs text-muted-foreground">
                      {artifact.size_bytes !== null
                        ? `${(artifact.size_bytes / 1024).toFixed(1)} KiB`
                        : "size unknown"}
                    </p>
                    {artifact.checksum_sha256 && (
                      <p className="truncate font-mono text-xs text-muted-foreground">
                        sha256:{artifact.checksum_sha256}
                      </p>
                    )}
                  </div>
                ))
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Timeline</CardTitle>
            </CardHeader>
            <CardContent>
              <ol className="space-y-2 text-sm">
                {detail.events.map((event) => (
                  <li key={event.id} className="flex gap-2">
                    <Badge variant="outline" className="mt-0.5 shrink-0 capitalize">
                      {event.kind}
                    </Badge>
                    <div>
                      <p>{event.message}</p>
                      <p className="text-xs text-muted-foreground">
                        {new Date(event.created_at).toLocaleString()}
                      </p>
                    </div>
                  </li>
                ))}
              </ol>
            </CardContent>
          </Card>

          {detail.reviews.length > 0 && (
            <Card>
              <CardHeader>
                <CardTitle>Reviews</CardTitle>
              </CardHeader>
              <CardContent className="space-y-2 text-sm">
                {detail.reviews.map((review) => (
                  <div key={review.id} className="rounded-md border p-2">
                    <p className="font-medium capitalize">{review.status}</p>
                    {review.reason && <p className="text-muted-foreground">{review.reason}</p>}
                    <p className="text-xs text-muted-foreground">
                      Requested {new Date(review.requested_at).toLocaleString()}
                    </p>
                  </div>
                ))}
              </CardContent>
            </Card>
          )}
        </div>
      </div>
    </div>
  );
}

function OverviewRow({
  label,
  value,
  mono,
  truncate,
}: {
  label: string;
  value: string;
  mono?: boolean;
  truncate?: boolean;
}) {
  return (
    <div className="flex justify-between gap-2">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className={`${mono ? "font-mono " : ""}${truncate ? "truncate " : ""}text-right`}>
        {value}
      </span>
    </div>
  );
}

export type { DeploymentDetail };
