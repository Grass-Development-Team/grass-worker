import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLinkIcon, RocketIcon } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
  formatDuration,
  isBuildRunning,
  shortCommit,
  type DeploymentEnvironment,
} from "./deployments.api";
import { BuildStatusBadge, ReleaseStatusBadge } from "./components/status-badges";

export function DeploymentsTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const [environmentFilter, setEnvironmentFilter] = useState<"all" | DeploymentEnvironment>("all");
  const [error, setError] = useState<string | null>(null);

  const deploymentsQuery = useQuery({
    queryKey: ["deployments", projectId, environmentFilter],
    queryFn: () =>
      deploymentsApi.list(
        projectId,
        environmentFilter === "all" ? undefined : { environment: environmentFilter },
      ),
    refetchInterval: (query) =>
      query.state.data?.deployments.some((deployment) => isBuildRunning(deployment.build_status))
        ? 4000
        : false,
  });

  const createMutation = useMutation({
    mutationFn: (environment: DeploymentEnvironment) =>
      deploymentsApi.create(projectId, { environment }),
    onSuccess: () => {
      setError(null);
      queryClient.invalidateQueries({ queryKey: ["deployments", projectId] });
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to create the deployment."),
  });

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
        <div className="flex gap-2">
          <Button
            variant="outline"
            onClick={() => createMutation.mutate("preview")}
            disabled={createMutation.isPending}
          >
            <RocketIcon /> Deploy preview
          </Button>
          <Button
            onClick={() => createMutation.mutate("production")}
            disabled={createMutation.isPending}
          >
            <RocketIcon /> Deploy production
          </Button>
        </div>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

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
            No deployments yet. Deploy production or preview to start a build.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Deployment</TableHead>
                <TableHead>Environment</TableHead>
                <TableHead>Build</TableHead>
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
                      {deployment.build_stage && isBuildRunning(deployment.build_status) && (
                        <p className="text-xs text-muted-foreground">{deployment.build_stage}</p>
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
