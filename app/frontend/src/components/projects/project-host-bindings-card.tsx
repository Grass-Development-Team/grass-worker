import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { ApiError } from "@/api/client";
import {
  adminPlatformHostSourcesQueryKey,
  createProjectHostBinding,
  deleteProjectHostBinding,
  getAdminPlatformHostSources,
  getProjectHostBindings,
  projectHostBindingsQueryKey,
  setProjectPrimaryHost,
  type PlatformHostSource,
  type ProjectHostBinding,
} from "@/api/hosts";
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

type ProjectHostBindingsCardProps = {
  currentUserIsAdmin: boolean;
  onHostsChanged?: () => Promise<void> | void;
  projectId: string;
};

type HostType = "custom_domain" | "platform_subdomain";

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }

  return fallback;
}

function upsertHost(
  current: ProjectHostBinding[] | undefined,
  nextHost: ProjectHostBinding,
): ProjectHostBinding[] {
  const remaining = current?.filter((host) => host.id !== nextHost.id) ?? [];

  if (nextHost.is_primary) {
    return [nextHost, ...remaining.map((host) => ({ ...host, is_primary: false }))];
  }

  return [nextHost, ...remaining];
}

function setPrimaryHostInList(
  current: ProjectHostBinding[] | undefined,
  nextHost: ProjectHostBinding,
): ProjectHostBinding[] {
  return (current ?? []).map((host) => ({
    ...host,
    is_primary: host.id === nextHost.id,
  }));
}

export function ProjectHostBindingsCard({
  currentUserIsAdmin,
  onHostsChanged,
  projectId,
}: ProjectHostBindingsCardProps) {
  const queryClient = useQueryClient();
  const [hostType, setHostType] = React.useState<HostType>("custom_domain");
  const [customHost, setCustomHost] = React.useState("");
  const [subdomainPrefix, setSubdomainPrefix] = React.useState("");
  const [selectedSourceId, setSelectedSourceId] = React.useState("");
  const [validationError, setValidationError] = React.useState<string | null>(null);
  const hostsQuery = useQuery({
    queryKey: projectHostBindingsQueryKey(projectId),
    queryFn: () => getProjectHostBindings(projectId),
  });
  const platformSourcesQuery = useQuery({
    queryKey: adminPlatformHostSourcesQueryKey,
    queryFn: getAdminPlatformHostSources,
    enabled: currentUserIsAdmin && hostType === "platform_subdomain",
  });

  const enabledPlatformSources = (platformSourcesQuery.data ?? []).filter(
    (source) => source.enabled,
  );

  React.useEffect(() => {
    if (!enabledPlatformSources.length) {
      setSelectedSourceId("");
      return;
    }

    if (!enabledPlatformSources.some((source) => source.id === selectedSourceId)) {
      setSelectedSourceId(enabledPlatformSources[0].id);
    }
  }, [enabledPlatformSources, selectedSourceId]);

  const runHostsChanged = async () => {
    await onHostsChanged?.();
  };

  const createMutation = useMutation({
    mutationFn: async () => {
      const existingHosts = hostsQuery.data ?? [];

      if (hostType === "platform_subdomain") {
        const source = enabledPlatformSources.find((item) => item.id === selectedSourceId);
        const prefix = subdomainPrefix.trim();

        if (!source) {
          throw new Error("A platform source is required");
        }

        if (!prefix) {
          throw new Error("Subdomain prefix is required");
        }

        return createProjectHostBinding(projectId, {
          host: `${prefix}.${source.base_domain}`,
          source_id: source.id,
          is_primary: existingHosts.length === 0,
        });
      }

      const host = customHost.trim();
      if (!host) {
        throw new Error("Host is required");
      }

      return createProjectHostBinding(projectId, {
        host,
        is_primary: existingHosts.length === 0,
      });
    },
    onSuccess: async (host) => {
      queryClient.setQueryData<ProjectHostBinding[]>(
        projectHostBindingsQueryKey(projectId),
        (current) => upsertHost(current, host),
      );
      setCustomHost("");
      setSubdomainPrefix("");
      setValidationError(null);
      await runHostsChanged();
    },
  });

  const setPrimaryMutation = useMutation({
    mutationFn: (bindingId: string) => setProjectPrimaryHost(projectId, bindingId),
    onSuccess: async (host) => {
      queryClient.setQueryData<ProjectHostBinding[]>(
        projectHostBindingsQueryKey(projectId),
        (current) => setPrimaryHostInList(current, host),
      );
      await runHostsChanged();
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (bindingId: string) => deleteProjectHostBinding(projectId, bindingId),
    onSuccess: async (_result, bindingId) => {
      queryClient.setQueryData<ProjectHostBinding[]>(
        projectHostBindingsQueryKey(projectId),
        (current) => (current ?? []).filter((host) => host.id !== bindingId),
      );
      await queryClient.invalidateQueries({
        queryKey: projectHostBindingsQueryKey(projectId),
      });
      await runHostsChanged();
    },
  });

  const mutationError =
    validationError ??
    (createMutation.isError
      ? errorMessage(createMutation.error, "Unable to bind host")
      : setPrimaryMutation.isError
        ? errorMessage(setPrimaryMutation.error, "Unable to update primary host")
        : deleteMutation.isError
          ? errorMessage(deleteMutation.error, "Unable to remove host")
          : null);

  const isMutating =
    createMutation.isPending || setPrimaryMutation.isPending || deleteMutation.isPending;

  const hostInputLabel = hostType === "platform_subdomain" ? "Subdomain prefix" : "Host";
  const hostInputValue = hostType === "platform_subdomain" ? subdomainPrefix : customHost;
  const hostInputPlaceholder =
    hostType === "platform_subdomain" ? "docs" : "docs.example.com";

  const setHostInputValue = (value: string) => {
    if (hostType === "platform_subdomain") {
      setSubdomainPrefix(value);
    } else {
      setCustomHost(value);
    }
  };

  const onSubmit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setValidationError(null);

    if (hostType === "platform_subdomain") {
      if (!selectedSourceId) {
        setValidationError("Platform source is required");
        return;
      }

      if (!subdomainPrefix.trim()) {
        setValidationError("Subdomain prefix is required");
        return;
      }
    } else if (!customHost.trim()) {
      setValidationError("Host is required");
      return;
    }

    createMutation.mutate();
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Host bindings</h2>
        </CardTitle>
        <CardDescription>
          Manage public hosts for this project and choose which one acts as the canonical URL.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        {hostsQuery.isError ? (
          <Alert variant="destructive">
            <AlertTitle>Unable to load host bindings</AlertTitle>
            <AlertDescription>
              {errorMessage(hostsQuery.error, "The host inventory request failed.")}
            </AlertDescription>
          </Alert>
        ) : hostsQuery.isPending ? (
          <div className="space-y-3">
            <Skeleton className="h-24" />
            <Skeleton className="h-24" />
          </div>
        ) : hostsQuery.data?.length ? (
          <div className="space-y-3">
            {hostsQuery.data.map((host) => (
              <Card key={host.id}>
                <CardHeader>
                  <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                    <div className="space-y-1">
                      <CardTitle>
                        <h3>{host.host}</h3>
                      </CardTitle>
                      <CardDescription>
                        {host.source_id ? "Platform-managed hostname" : "Custom domain"}
                      </CardDescription>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      {host.is_primary ? <Badge>Primary</Badge> : null}
                      {host.source_id ? <Badge variant="outline">Platform source</Badge> : null}
                    </div>
                  </div>
                </CardHeader>
                <CardContent className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                  <p className="text-sm text-muted-foreground">
                    {host.is_primary
                      ? "This host is currently used as the live site URL."
                      : "Promote this host to make it the canonical live site URL."}
                  </p>
                  <div className="flex flex-wrap gap-2">
                    {!host.is_primary ? (
                      <Button
                        aria-label={`Set ${host.host} as primary`}
                        disabled={isMutating}
                        onClick={() => setPrimaryMutation.mutate(host.id)}
                        type="button"
                        variant="outline"
                      >
                        Set as primary
                      </Button>
                    ) : null}
                    <Button
                      aria-label={`Remove ${host.host}`}
                      disabled={isMutating}
                      onClick={() => deleteMutation.mutate(host.id)}
                      type="button"
                      variant="outline"
                    >
                      Remove host
                    </Button>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>
        ) : (
          <Card>
            <CardHeader>
              <CardTitle>No hosts bound yet</CardTitle>
              <CardDescription>
                Add a custom domain or platform hostname to make this project reachable.
              </CardDescription>
            </CardHeader>
          </Card>
        )}

        <form className="space-y-4" onSubmit={onSubmit}>
          {currentUserIsAdmin ? (
            <div className="space-y-2">
              <Label htmlFor="project-host-type">Host type</Label>
              <select
                className="flex h-10 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
                id="project-host-type"
                onChange={(event) => {
                  setHostType(event.target.value as HostType);
                  setValidationError(null);
                }}
                value={hostType}
              >
                <option value="custom_domain">Custom domain</option>
                <option value="platform_subdomain">Platform subdomain</option>
              </select>
            </div>
          ) : null}

          {hostType === "platform_subdomain" ? (
            <div className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="project-platform-source">Platform source</Label>
                <select
                  className="flex h-10 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
                  disabled={!enabledPlatformSources.length || platformSourcesQuery.isPending}
                  id="project-platform-source"
                  onChange={(event) => {
                    setSelectedSourceId(event.target.value);
                    setValidationError(null);
                  }}
                  value={selectedSourceId}
                >
                  {enabledPlatformSources.length ? null : (
                    <option value="">No enabled platform sources</option>
                  )}
                  {enabledPlatformSources.map((source) => (
                    <option key={source.id} value={source.id}>
                      {source.label} ({source.base_domain})
                    </option>
                  ))}
                </select>
              </div>
              {!platformSourcesQuery.isPending && !enabledPlatformSources.length ? (
                <Alert>
                  <AlertTitle>No platform sources available</AlertTitle>
                  <AlertDescription>
                    Create or enable a platform host source before assigning platform
                    subdomains to this project.
                  </AlertDescription>
                </Alert>
              ) : null}
            </div>
          ) : null}

          <div className="space-y-2">
            <Label htmlFor="project-host-input">{hostInputLabel}</Label>
            <Input
              id="project-host-input"
              onChange={(event) => {
                setHostInputValue(event.target.value);
                setValidationError(null);
              }}
              placeholder={hostInputPlaceholder}
              value={hostInputValue}
            />
          </div>

          {hostType === "platform_subdomain" ? (
            <p className="text-sm text-muted-foreground">
              The selected prefix is combined with the chosen base domain to form the final
              host.
            </p>
          ) : null}

          {mutationError ? (
            <Alert variant="destructive">
              <AlertTitle>Host update failed</AlertTitle>
              <AlertDescription>{mutationError}</AlertDescription>
            </Alert>
          ) : null}

          <Button
            disabled={
              isMutating ||
              (hostType === "platform_subdomain" &&
                (platformSourcesQuery.isPending || !enabledPlatformSources.length))
            }
            type="submit"
          >
            {createMutation.isPending ? "Adding host..." : "Add host"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
