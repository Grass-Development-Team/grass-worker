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
  const [newRegion, setNewRegion] = useState("default");

  const hostsQuery = useQuery({
    queryKey: ["project-hosts", projectId],
    queryFn: () => projectsApi.listHosts(projectId),
  });

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["project-hosts", projectId] });

  const addMutation = useMutation({
    mutationFn: () => projectsApi.createHost(projectId, { host: newHost, region: newRegion }),
    onSuccess: () => {
      setNewHost("");
      setNewRegion("default");
      invalidate();
    },
  });
  const removeMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.removeHost(projectId, hostId),
    onSuccess: invalidate,
  });
  const primaryMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.setPrimaryHost(projectId, hostId),
    onSuccess: invalidate,
  });
  const provisionMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.provisionHost(projectId, hostId),
    onSuccess: invalidate,
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
          className="flex flex-wrap items-end gap-2"
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
          <Field className="w-36">
            <FieldLabel htmlFor="new-host-region">Region</FieldLabel>
            <Input
              id="new-host-region"
              placeholder="default"
              value={newRegion}
              onChange={(event) => setNewRegion(event.target.value)}
            />
          </Field>
          <Button type="submit" disabled={addMutation.isPending || !newHost.trim()}>
            <PlusIcon /> Add
          </Button>
        </form>
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
                <TableHead>Region</TableHead>
                <TableHead>Environment</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Serving</TableHead>
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
                    {host.ingress && (
                      <details className="mt-2 text-xs text-muted-foreground">
                        <summary className="cursor-pointer">DNS configuration</summary>
                        <div className="mt-1 flex flex-col gap-1 font-mono">
                          <span>
                            CNAME {host.ingress.cname.name} → {host.ingress.cname.target}
                          </span>
                          <span>
                            TXT {host.ingress.txt.name} → {host.ingress.txt.value}
                          </span>
                        </div>
                        <p className="mt-1 font-sans">
                          TLS {host.ingress.certificate.status} · DNS-01{" "}
                          {host.ingress.dns_challenge.status} · {host.ingress.entrance_nodes.length}{" "}
                          healthy entrance node{host.ingress.entrance_nodes.length === 1 ? "" : "s"}
                        </p>
                      </details>
                    )}
                  </TableCell>
                  <TableCell className="capitalize">{host.kind}</TableCell>
                  <TableCell className="font-mono text-sm">{host.region ?? "default"}</TableCell>
                  <TableCell className="capitalize">{host.environment}</TableCell>
                  <TableCell>
                    <Badge variant={hostStatusVariant(host.status)}>{host.status}</Badge>
                  </TableCell>
                  <TableCell>
                    <Badge variant={host.serving ? "success" : "secondary"}>
                      {host.serving
                        ? "Serving"
                        : host.status === "active"
                          ? "Bound · no deployment"
                          : "Not serving"}
                    </Badge>
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
