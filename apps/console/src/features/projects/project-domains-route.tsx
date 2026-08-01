import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { GlobeIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { canContributeToProjects } from "@/features/teams/team-permissions";

import { projectsApi, type HostStatus } from "./projects.api";
import { useProject } from "./project-layout";

export function hostStatusVariant(
  status: HostStatus,
): "success" | "warning" | "destructive" | "secondary" {
  switch (status) {
    case "active":
      return "success";
    case "pending":
      return "warning";
    case "failed":
      return "destructive";
    case "disabled":
      return "secondary";
  }
}

export function ProjectDomainsRoute() {
  const { project, role } = useProject();
  const projectId = project.id;
  const canEdit = canContributeToProjects(role);
  const queryClient = useQueryClient();
  const [newHost, setNewHost] = useState("");
  const [error, setError] = useState<string | null>(null);

  const hostsQuery = useQuery({
    queryKey: ["project-hosts", projectId],
    queryFn: () => projectsApi.listHosts(projectId),
  });

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["project-hosts", projectId] });

  const addMutation = useMutation({
    mutationFn: () => projectsApi.createHost(projectId, { host: newHost }),
    onSuccess: () => {
      setNewHost("");
      setError(null);
      invalidate();
    },
    onError: (cause) => setError(cause instanceof Error ? cause.message : "Unable to add domain."),
  });
  const removeMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.removeHost(projectId, hostId),
    onSuccess: invalidate,
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to remove domain."),
  });
  const primaryMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.setPrimaryHost(projectId, hostId),
    onSuccess: invalidate,
    onError: (cause) => setError(cause instanceof Error ? cause.message : "Unable to set primary."),
  });
  const provisionMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.provisionHost(projectId, hostId),
    onSuccess: invalidate,
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to retry provisioning."),
  });

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-lg font-semibold">Domains</h1>
        <p className="text-sm text-muted-foreground">
          Platform domains are assigned automatically; custom domains must point at the platform
          nodes.
        </p>
      </div>

      {canEdit && (
        <form
          className="flex items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (newHost.trim()) addMutation.mutate();
          }}
        >
          <Field className="max-w-sm flex-1">
            <FieldLabel htmlFor="new-host">Add domain</FieldLabel>
            <Input
              id="new-host"
              placeholder="app.example.com"
              value={newHost}
              onChange={(event) => setNewHost(event.target.value)}
            />
          </Field>
          <Button type="submit" disabled={addMutation.isPending || !newHost.trim()}>
            <PlusIcon /> Add
          </Button>
        </form>
      )}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      {hostsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {hostsQuery.data &&
        (hostsQuery.data.hosts.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <GlobeIcon className="mr-1 inline size-4" />
            No domains yet. Platform domains are assigned automatically when a host source is
            configured.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Domain</TableHead>
                <TableHead>Kind</TableHead>
                <TableHead>Environment</TableHead>
                <TableHead>Status</TableHead>
                {canEdit && <TableHead className="text-right">Actions</TableHead>}
              </TableRow>
            </TableHeader>
            <TableBody>
              {hostsQuery.data.hosts.map((host) => (
                <TableRow key={host.id}>
                  <TableCell>
                    <span className="font-medium">{host.host}</span>
                    {host.is_primary && (
                      <Badge variant="outline" className="ml-2">
                        Primary
                      </Badge>
                    )}
                    {host.failure_reason && (
                      <p className="text-xs text-destructive">{host.failure_reason}</p>
                    )}
                  </TableCell>
                  <TableCell className="capitalize">{host.kind}</TableCell>
                  <TableCell className="capitalize">{host.environment}</TableCell>
                  <TableCell>
                    <Badge variant={hostStatusVariant(host.status)}>{host.status}</Badge>
                  </TableCell>
                  {canEdit && (
                    <TableCell className="space-x-1 text-right">
                      {(host.status === "pending" || host.status === "failed") &&
                        host.host_source_id && (
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => provisionMutation.mutate(host.id)}
                            disabled={provisionMutation.isPending}
                          >
                            Retry
                          </Button>
                        )}
                      {!host.is_primary && (
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => primaryMutation.mutate(host.id)}
                          disabled={primaryMutation.isPending}
                        >
                          Make primary
                        </Button>
                      )}
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={`Remove ${host.host}`}
                        onClick={() => removeMutation.mutate(host.id)}
                        disabled={removeMutation.isPending}
                      >
                        <Trash2Icon />
                      </Button>
                    </TableCell>
                  )}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}
    </div>
  );
}
