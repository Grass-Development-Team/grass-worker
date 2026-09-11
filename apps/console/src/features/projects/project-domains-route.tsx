import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { GlobeIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { RegionSelect } from "@/features/regions/region-select";
import { regionsApi } from "@/features/regions/regions.api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
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
import { DomainCertificateControls } from "./domain-certificate-controls";

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
  const [newRegion, setNewRegion] = useState("");

  const regionsQuery = useQuery({ queryKey: ["regions"], queryFn: regionsApi.list });
  const regionAvailable =
    regionsQuery.data?.regions.some(
      (region) => region.code === newRegion && region.ingress_hostname && region.ingress_enabled,
    ) ?? false;
  const hostsQuery = useQuery({
    queryKey: ["project-hosts", projectId],
    queryFn: () => projectsApi.listHosts(projectId),
    refetchInterval: 10_000,
  });

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["project-hosts", projectId] });

  const addMutation = useMutation({
    mutationFn: () => projectsApi.createHost(projectId, { host: newHost, region: newRegion }),
    onSuccess: () => {
      setNewHost("");
      setNewRegion("");
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
  const verifyMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.verifyHost(projectId, hostId),
    onSuccess: invalidate,
  });

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h1 className="text-lg font-semibold">Domains</h1>
        <p className="text-sm text-muted-foreground">
          Add your DNS records. The server checks the connection every 1–2 minutes and sets up HTTPS
          automatically.
        </p>
      </div>

      {canEdit && (
        <form
          className="flex flex-wrap items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (newHost.trim() && regionAvailable) addMutation.mutate();
          }}
        >
          <FieldGroup className="max-w-lg">
            <Field className="max-w-sm flex-1">
              <FieldLabel htmlFor="new-host">Add domain</FieldLabel>
              <Input
                id="new-host"
                placeholder="app.example.com"
                value={newHost}
                onChange={(event) => setNewHost(event.target.value)}
              />
            </Field>
            <Field className="max-w-sm">
              <FieldLabel htmlFor="new-host-region">Region</FieldLabel>
              <RegionSelect
                id="new-host-region"
                value={newRegion}
                onChange={setNewRegion}
                requireIngress
              />
            </Field>
          </FieldGroup>
          <Button
            type="submit"
            disabled={addMutation.isPending || !newHost.trim() || !regionAvailable}
          >
            <PlusIcon data-icon="inline-start" /> Add
          </Button>
        </form>
      )}
      {(addMutation.isError ||
        removeMutation.isError ||
        verifyMutation.isError ||
        hostsQuery.isError) && (
        <Alert variant="destructive">
          <AlertDescription>
            {addMutation.error?.message ??
              removeMutation.error?.message ??
              verifyMutation.error?.message ??
              "Domains could not be loaded."}
          </AlertDescription>
        </Alert>
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
                <TableHead>HTTPS</TableHead>
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
                    {host.kind === "custom" && (
                      <div className="mt-1 flex flex-col gap-1">
                        <Badge
                          variant={host.connection_state === "ready" ? "success" : "secondary"}
                        >
                          {connectionLabel(host.connection_state)}
                        </Badge>
                        {host.onboarding?.dns_error && (
                          <p className="text-xs text-destructive">{host.onboarding.dns_error}</p>
                        )}
                        {host.ownership_error && (
                          <p className="text-xs text-muted-foreground">{host.ownership_error}</p>
                        )}
                        {host.onboarding?.checked_at && (
                          <p className="text-xs text-muted-foreground">
                            Last checked {new Date(host.onboarding.checked_at).toLocaleString()}
                          </p>
                        )}
                        {host.onboarding?.next_check_at && host.connection_state !== "ready" && (
                          <p className="text-xs text-muted-foreground">
                            Next check{" "}
                            {new Date(host.onboarding.next_check_at).toLocaleTimeString()}. Checks
                            continue when this page is closed.
                          </p>
                        )}
                        {host.connection_state === "entry_unavailable" && (
                          <p className="text-xs text-muted-foreground">
                            The platform entry is not ready. Contact an administrator; checks resume
                            automatically when it is available.
                          </p>
                        )}
                      </div>
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
                          Keep these records for automatic certificate renewal. Root domains require
                          CNAME flattening or an equivalent DNS feature.
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
                    <DomainCertificateControls
                      host={host}
                      projectId={projectId}
                      canEdit={canEdit}
                      onChange={() => {
                        void invalidate();
                      }}
                    />
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
                    <TableCell>
                      <div className="flex flex-wrap justify-end gap-2">
                        {host.kind === "custom" && (
                          <Button
                            size="sm"
                            variant="outline"
                            disabled={verifyMutation.isPending}
                            onClick={() => verifyMutation.mutate(host.id)}
                          >
                            Check now
                          </Button>
                        )}
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
                          <Trash2Icon data-icon="inline-start" />
                        </Button>
                      </div>
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

function connectionLabel(state?: string): string {
  const labels: Record<string, string> = {
    pending: "Checking DNS",
    unresolved: "DNS record missing",
    mismatch: "Incorrect DNS target",
    error: "DNS check will retry",
    entry_unavailable: "Platform entry not ready",
    ownership_pending: "Waiting for ownership TXT",
    review_pending: "Waiting for domain review",
    certificate_pending: "Waiting for certificate",
    issuing: "Issuing certificate",
    installing: "Installing certificate",
    ready: "Connected",
    certificate_failed: "Certificate retry scheduled",
    disabled: "Disabled",
    contact_missing: "Re-add domain to set its contact account",
  };
  return labels[state ?? "pending"] ?? "Checking connection";
}
