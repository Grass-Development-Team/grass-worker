import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { useOutletContext } from "react-router-dom";
import {
  adminPlatformHostSourcesQueryKey,
  createPlatformHostSource,
  disablePlatformHostSource,
  enablePlatformHostSource,
  getAdminPlatformHostSources,
  type CreatePlatformHostSourceInput,
  type PlatformHostSource,
  type PlatformHostSourceKind,
} from "@/api/hosts";
import { ConsolePageHeader } from "@/components/console/console-page-header";
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
import type { ProtectedOutletContext } from "./protected-route";

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) {
    return error.message;
  }

  return fallback;
}

function sourceKindLabel(kind: PlatformHostSourceKind) {
  if (kind === "dns_managed") {
    return "DNS managed";
  }

  return "Wildcard static";
}

function upsertSource(
  current: PlatformHostSource[] | undefined,
  source: PlatformHostSource,
): PlatformHostSource[] {
  const next = current?.filter((item) => item.id !== source.id) ?? [];
  return [source, ...next];
}

type CreateSourceFormProps = {
  error: string | null;
  isCreating: boolean;
  onCreate: (input: CreatePlatformHostSourceInput) => void;
  onResetError: () => void;
  resetToken: number;
};

function CreateSourceCard({
  error,
  isCreating,
  onCreate,
  onResetError,
  resetToken,
}: CreateSourceFormProps) {
  const [kind, setKind] = React.useState<PlatformHostSourceKind>("wildcard_static");
  const [label, setLabel] = React.useState("");
  const [baseDomain, setBaseDomain] = React.useState("");
  const [enabled, setEnabled] = React.useState(true);
  const [allowsAutoAssign, setAllowsAutoAssign] = React.useState(true);
  const [validationError, setValidationError] = React.useState<string | null>(null);
  const createError = validationError ?? error;

  React.useEffect(() => {
    setKind("wildcard_static");
    setLabel("");
    setBaseDomain("");
    setEnabled(true);
    setAllowsAutoAssign(true);
    setValidationError(null);
  }, [resetToken]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Create platform host source</h2>
        </CardTitle>
        <CardDescription>
          Register a reusable base domain that projects can bind as a platform hostname.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();

            const nextLabel = label.trim();
            const nextBaseDomain = baseDomain.trim();

            if (!nextLabel) {
              setValidationError("Label is required");
              return;
            }

            if (!nextBaseDomain) {
              setValidationError("Base domain is required");
              return;
            }

            setValidationError(null);
            onCreate({
              kind,
              label: nextLabel,
              base_domain: nextBaseDomain,
              enabled,
              allows_auto_assign: allowsAutoAssign,
            });
          }}
        >
          <div className="space-y-2">
            <Label htmlFor="platform-host-source-label">Label</Label>
            <Input
              id="platform-host-source-label"
              onChange={(event) => {
                setLabel(event.target.value);
                setValidationError(null);
                onResetError();
              }}
              placeholder="Primary Sites"
              value={label}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="platform-host-source-base-domain">Base domain</Label>
            <Input
              id="platform-host-source-base-domain"
              onChange={(event) => {
                setBaseDomain(event.target.value);
                setValidationError(null);
                onResetError();
              }}
              placeholder="apps.example.com"
              value={baseDomain}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="platform-host-source-kind">Kind</Label>
            <select
              className="flex h-10 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
              id="platform-host-source-kind"
              onChange={(event) => {
                setKind(event.target.value as PlatformHostSourceKind);
                setValidationError(null);
                onResetError();
              }}
              value={kind}
            >
              <option value="wildcard_static">Wildcard static</option>
              <option value="dns_managed">DNS managed</option>
            </select>
          </div>
          <label className="flex items-center gap-3 text-sm font-medium" htmlFor="enabled">
            <input
              checked={enabled}
              id="enabled"
              onChange={(event) => {
                setEnabled(event.target.checked);
                onResetError();
              }}
              type="checkbox"
            />
            Enabled
          </label>
          <label
            className="flex items-center gap-3 text-sm font-medium"
            htmlFor="allows-auto-assign"
          >
            <input
              checked={allowsAutoAssign}
              id="allows-auto-assign"
              onChange={(event) => {
                setAllowsAutoAssign(event.target.checked);
                onResetError();
              }}
              type="checkbox"
            />
            Allow auto-assign
          </label>
          {createError ? (
            <Alert variant="destructive">
              <AlertTitle>Source creation failed</AlertTitle>
              <AlertDescription>{createError}</AlertDescription>
            </Alert>
          ) : null}
          <Button className="w-full" disabled={isCreating} type="submit">
            {isCreating ? "Creating source..." : "Create source"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}

export function AdminPlatformHostSourcesPage() {
  const { currentUser } = useOutletContext<ProtectedOutletContext>();
  const queryClient = useQueryClient();
  const [createResetToken, setCreateResetToken] = React.useState(0);
  const sourcesQuery = useQuery({
    queryKey: adminPlatformHostSourcesQueryKey,
    queryFn: getAdminPlatformHostSources,
  });

  const createMutation = useMutation({
    mutationFn: createPlatformHostSource,
    onSuccess: async (source) => {
      queryClient.setQueryData<PlatformHostSource[]>(
        adminPlatformHostSourcesQueryKey,
        (current) => upsertSource(current, source),
      );
      setCreateResetToken((current) => current + 1);
    },
  });

  const toggleMutation = useMutation({
    mutationFn: (input: { sourceId: string; enabled: boolean }) =>
      input.enabled
        ? enablePlatformHostSource(input.sourceId)
        : disablePlatformHostSource(input.sourceId),
    onSuccess: async (source) => {
      queryClient.setQueryData<PlatformHostSource[]>(
        adminPlatformHostSourcesQueryKey,
        (current) => upsertSource(current, source),
      );
    },
  });

  return (
    <div className="space-y-6">
      <ConsolePageHeader
        actions={
          <Button
            disabled={sourcesQuery.isPending}
            onClick={() => void sourcesQuery.refetch()}
            type="button"
            variant="outline"
          >
            {sourcesQuery.isPending ? "Refreshing..." : "Refresh sources"}
          </Button>
        }
        description={`Manage reusable host inventories available to ${currentUser.email}.`}
        eyebrow="Admin"
        title="Platform host sources"
      />

      {sourcesQuery.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Unable to load platform host sources</AlertTitle>
          <AlertDescription>
            {errorMessage(sourcesQuery.error, "The host source inventory request failed.")}
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-6 xl:grid-cols-[minmax(0,2fr)_minmax(360px,1fr)]">
        <Card>
          <CardHeader>
            <CardTitle>Source inventory</CardTitle>
            <CardDescription>
              Enabled sources can back platform subdomains for project host bindings.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {sourcesQuery.isPending ? (
              <div className="space-y-3">
                <Skeleton className="h-28" />
                <Skeleton className="h-28" />
              </div>
            ) : sourcesQuery.data?.length ? (
              sourcesQuery.data.map((source) => (
                <Card key={source.id}>
                  <CardHeader>
                    <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                      <div className="space-y-1">
                        <CardTitle>
                          <h2>{source.label}</h2>
                        </CardTitle>
                        <CardDescription>{source.base_domain}</CardDescription>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <Badge variant={source.enabled ? "default" : "secondary"}>
                          {source.enabled ? "Enabled" : "Disabled"}
                        </Badge>
                        <Badge variant="outline">{sourceKindLabel(source.kind)}</Badge>
                        {source.allows_auto_assign ? (
                          <Badge variant="outline">Auto-assign</Badge>
                        ) : null}
                      </div>
                    </div>
                  </CardHeader>
                  <CardContent className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                    <p className="text-sm text-muted-foreground">
                      {source.enabled
                        ? "Projects can bind this source immediately."
                        : "Disabled sources stay visible but cannot be newly assigned."}
                    </p>
                    <Button
                      disabled={toggleMutation.isPending}
                      onClick={() =>
                        toggleMutation.mutate({
                          sourceId: source.id,
                          enabled: !source.enabled,
                        })
                      }
                      type="button"
                      variant="outline"
                    >
                      {source.enabled ? "Disable source" : "Enable source"}
                    </Button>
                  </CardContent>
                </Card>
              ))
            ) : (
              <Card>
                <CardHeader>
                  <CardTitle>No platform host sources yet</CardTitle>
                  <CardDescription>
                    Create the first source to make platform-managed subdomains available.
                  </CardDescription>
                </CardHeader>
              </Card>
            )}
          </CardContent>
        </Card>

        <CreateSourceCard
          error={
            createMutation.isError
              ? errorMessage(createMutation.error, "Unable to create source")
              : null
          }
          isCreating={createMutation.isPending}
          onCreate={(input) => createMutation.mutate(input)}
          onResetError={() => createMutation.reset()}
          resetToken={createResetToken}
        />
      </div>
    </div>
  );
}
