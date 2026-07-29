import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLinkIcon, RocketIcon } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldLabel } from "@/components/ui/field";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import {
  deploymentsApi,
  deploymentRefetchInterval,
  formatDuration,
  isBuildRunning,
  shortCommit,
  type DeploymentEnvironment,
  type ServeNodeTarget,
} from "./deployments.api";
import { BuildStatusBadge, ReleaseStatusBadge, ServeStatusBadge } from "./components/status-badges";

export function DeploymentsTab({
  projectId,
  canDeploy,
}: {
  projectId: string;
  canDeploy: boolean;
}) {
  const queryClient = useQueryClient();
  const [environmentFilter, setEnvironmentFilter] = useState<"all" | DeploymentEnvironment>("all");
  const [deploymentEnvironment, setDeploymentEnvironment] = useState<DeploymentEnvironment | null>(
    null,
  );
  const [serveNodeId, setServeNodeId] = useState("automatic");
  const [error, setError] = useState<string | null>(null);

  const deploymentsQuery = useQuery({
    queryKey: ["deployments", projectId, environmentFilter],
    queryFn: () =>
      deploymentsApi.list(
        projectId,
        environmentFilter === "all" ? undefined : { environment: environmentFilter },
      ),
    refetchInterval: (query) =>
      query.state.data?.deployments.some(
        (deployment) => deploymentRefetchInterval(deployment) !== false,
      )
        ? 4000
        : false,
  });

  const serveNodesQuery = useQuery({
    queryKey: ["serve-nodes", projectId],
    queryFn: () => deploymentsApi.serveNodes(projectId),
    enabled: canDeploy && deploymentEnvironment !== null,
  });

  const createMutation = useMutation({
    mutationFn: (input: { environment: DeploymentEnvironment; serve_node_id?: string }) =>
      deploymentsApi.create(projectId, input),
    onSuccess: () => {
      setError(null);
      setDeploymentEnvironment(null);
      queryClient.invalidateQueries({ queryKey: ["deployments", projectId] });
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to create the deployment."),
  });

  const serveNodes = serveNodesQuery.data?.serve_nodes ?? [];
  const selectedNode = serveNodes.find((node) => node.id === serveNodeId);
  const canSubmit =
    !serveNodesQuery.isLoading &&
    !serveNodesQuery.isError &&
    (serveNodeId === "automatic"
      ? serveNodes.some((node) => node.schedulable)
      : selectedNode?.schedulable === true);

  const openDeploymentDialog = (environment: DeploymentEnvironment) => {
    setServeNodeId("automatic");
    setError(null);
    setDeploymentEnvironment(environment);
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Select
          value={environmentFilter}
          onValueChange={(value) => setEnvironmentFilter(value as never)}
        >
          <SelectTrigger className="w-44" aria-label="Filter environment">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All environments</SelectItem>
            <SelectItem value="production">Production</SelectItem>
            <SelectItem value="preview">Preview</SelectItem>
          </SelectContent>
        </Select>
        {canDeploy && (
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={() => openDeploymentDialog("preview")}
              disabled={createMutation.isPending}
            >
              <RocketIcon /> Deploy preview
            </Button>
            <Button
              onClick={() => openDeploymentDialog("production")}
              disabled={createMutation.isPending}
            >
              <RocketIcon /> Deploy production
            </Button>
          </div>
        )}
      </div>
      <Dialog
        open={deploymentEnvironment !== null}
        onOpenChange={(open) => {
          if (!open) setDeploymentEnvironment(null);
        }}
      >
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle className="capitalize">Deploy {deploymentEnvironment}</DialogTitle>
            <DialogDescription>Serve placement for this deployment.</DialogDescription>
          </DialogHeader>
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              if (!deploymentEnvironment || !canSubmit) return;
              createMutation.mutate({
                environment: deploymentEnvironment,
                ...(serveNodeId === "automatic" ? {} : { serve_node_id: serveNodeId }),
              });
            }}
          >
            <Field>
              <FieldLabel htmlFor="serve-node">Serve node</FieldLabel>
              <Select value={serveNodeId} onValueChange={setServeNodeId}>
                <SelectTrigger id="serve-node" aria-label="Serve node" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    value="automatic"
                    disabled={!serveNodes.some((node) => node.schedulable)}
                  >
                    Automatic · least loaded
                  </SelectItem>
                  {serveNodes.map((node) => (
                    <SelectItem key={node.id} value={node.id} disabled={!node.schedulable}>
                      {node.name} · {formatNodeUsage(node)}
                      {node.overflow_only ? " · overflow" : ""}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            {serveNodesQuery.isLoading && (
              <p className="text-sm text-muted-foreground">Loading Serve Nodes…</p>
            )}
            {serveNodesQuery.isError && (
              <p role="alert" className="text-sm text-destructive">
                {serveNodesQuery.error instanceof Error
                  ? serveNodesQuery.error.message
                  : "Unable to load Serve Nodes."}
              </p>
            )}
            {serveNodesQuery.data && !serveNodes.some((node) => node.schedulable) && (
              <p role="alert" className="text-sm text-destructive">
                No Serve Node can accept this deployment.
              </p>
            )}
            {createMutation.isError && error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            <DialogFooter>
              <Button type="submit" disabled={!canSubmit || createMutation.isPending}>
                {createMutation.isPending ? "Creating…" : "Create deployment"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      {deploymentsQuery.isLoading && <Skeleton className="h-64 w-full" aria-busy="true" />}
      {deploymentsQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {deploymentsQuery.error instanceof Error
            ? deploymentsQuery.error.message
            : "Unable to load deployments."}
        </p>
      )}
      {deploymentsQuery.data &&
        (deploymentsQuery.data.deployments.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {canDeploy
              ? "No deployments yet. Deploy production or preview to start a build."
              : "No deployments yet."}
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Deployment</TableHead>
                <TableHead>Environment</TableHead>
                <TableHead>Build</TableHead>
                <TableHead>Serve</TableHead>
                <TableHead>Release</TableHead>
                <TableHead>Source</TableHead>
                <TableHead>Duration</TableHead>
                <TableHead>URL</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {deploymentsQuery.data.deployments.map((deployment) => {
                const url = deployment.production_url ?? deployment.preview_url;
                return (
                  <TableRow key={deployment.id}>
                    <TableCell>
                      <Link
                        to={`/projects/${projectId}/deployments/${deployment.id}`}
                        className="font-mono text-xs font-medium hover:underline"
                      >
                        {deployment.id.slice(0, 8)}
                      </Link>
                      <p className="text-xs text-muted-foreground">
                        {new Date(deployment.created_at).toLocaleString()}
                        {deployment.triggered_by
                          ? ` · ${deployment.triggered_by.display_name ?? deployment.triggered_by.email}`
                          : ""}
                      </p>
                      {deployment.failure_message && (
                        <p className="max-w-64 truncate text-xs text-destructive">
                          {deployment.failure_message}
                        </p>
                      )}
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline" className="capitalize">
                        {deployment.environment}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <BuildStatusBadge status={deployment.build_status} />
                      {deployment.build_node && (
                        <p className="text-xs text-muted-foreground">
                          {deployment.build_node.name}
                        </p>
                      )}
                      {deployment.build_stage && isBuildRunning(deployment.build_status) && (
                        <p className="text-xs text-muted-foreground">{deployment.build_stage}</p>
                      )}
                    </TableCell>
                    <TableCell>
                      <div className="flex flex-wrap items-center gap-1">
                        <ServeStatusBadge status={deployment.serve_status} />
                        {deployment.overcommitted && <Badge variant="warning">Overflow</Badge>}
                      </div>
                      <p className="text-xs text-muted-foreground">
                        {deployment.serve_node?.name ?? "Unassigned"}
                      </p>
                      <p className="whitespace-nowrap text-xs text-muted-foreground">
                        {deployment.serve_resources.cpu_millicores}m ·{" "}
                        {deployment.serve_resources.memory_mb}
                        MB · {deployment.serve_resources.disk_mb} MB disk
                      </p>
                      {deployment.serve_failure_message && (
                        <p className="max-w-64 truncate text-xs text-destructive">
                          {deployment.serve_failure_message}
                        </p>
                      )}
                    </TableCell>
                    <TableCell>
                      <ReleaseStatusBadge status={deployment.release_status} />
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      <p>{deployment.source.branch ?? "—"}</p>
                      <p className="font-mono">{shortCommit(deployment.source.commit_hash)}</p>
                    </TableCell>
                    <TableCell className="text-sm tabular-nums text-muted-foreground">
                      {formatDuration(deployment.duration_seconds)}
                    </TableCell>
                    <TableCell>
                      {url ? (
                        <a
                          href={url}
                          target="_blank"
                          rel="noreferrer"
                          className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
                        >
                          Visit <ExternalLinkIcon className="size-3" />
                        </a>
                      ) : (
                        <span className="text-xs text-muted-foreground">—</span>
                      )}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        ))}
    </div>
  );
}

function formatNodeUsage(node: ServeNodeTarget): string {
  return `${node.usage.cpu_millicores}/${node.capacity.cpu_millicores}m · ${node.usage.memory_mb}/${node.capacity.memory_mb} MB · ${node.usage.disk_mb}/${node.capacity.disk_mb} MB disk · ${node.usage.deployments}/${node.capacity.max_deployments} deployments`;
}
