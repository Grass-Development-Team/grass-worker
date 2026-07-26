import { useQuery } from "@tanstack/react-query";
import { ExternalLinkIcon, GitBranchIcon, GlobeIcon, RocketIcon } from "lucide-react";
import { Link } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import {
  deploymentsApi,
  shortCommit,
  type BuildStatus,
  type Deployment,
} from "@/features/deployments/deployments.api";
import {
  BuildStatusBadge,
  ReleaseStatusBadge,
} from "@/features/deployments/components/status-badges";

import { projectsApi } from "./projects.api";
import { useProject } from "./project-layout";

function statusDotClass(status: BuildStatus): string {
  switch (status) {
    case "ready":
      return "bg-emerald-500";
    case "failed":
      return "bg-destructive";
    case "canceled":
      return "bg-muted-foreground";
    default:
      return "bg-amber-500";
  }
}

function OverviewRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <p className="text-xs text-muted-foreground">{label}</p>
      <div className="text-sm">{children}</div>
    </div>
  );
}

export function ProjectOverviewRoute() {
  const { project } = useProject();

  const deploymentsQuery = useQuery({
    queryKey: ["deployments", project.id, "production"],
    queryFn: () => deploymentsApi.list(project.id, { environment: "production" }),
  });
  const hostsQuery = useQuery({
    queryKey: ["project-hosts", project.id],
    queryFn: () => projectsApi.listHosts(project.id),
  });

  const deployments = deploymentsQuery.data?.deployments ?? [];
  const production: Deployment | null =
    deployments.find((deployment) => deployment.release_status === "active") ??
    deployments[0] ??
    null;
  const hosts = hostsQuery.data?.hosts ?? [];
  const productionHosts = hosts.filter((host) => host.environment !== "preview");
  const primaryHost = hosts.find((host) => host.is_primary) ?? productionHosts[0] ?? null;
  const visitUrl = production?.production_url ?? production?.preview_url ?? null;

  return (
    <div className="space-y-6">
      <Card className="gap-0 overflow-hidden py-0">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b px-6 py-3.5">
          <h1 className="text-sm font-medium">Production Deployment</h1>
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" asChild>
              <Link to={`/projects/${project.id}/deployments`}>Deployments</Link>
            </Button>
            {visitUrl && (
              <Button size="sm" asChild>
                <a href={visitUrl} target="_blank" rel="noreferrer">
                  Visit <ExternalLinkIcon />
                </a>
              </Button>
            )}
          </div>
        </div>
        <CardContent className="px-6 py-6">
          {deploymentsQuery.isLoading && <Skeleton className="h-48 w-full" aria-busy="true" />}
          {deploymentsQuery.isError && (
            <p role="alert" className="text-sm text-destructive">
              {deploymentsQuery.error instanceof Error
                ? deploymentsQuery.error.message
                : "Unable to load deployments."}
            </p>
          )}
          {deploymentsQuery.data &&
            (production ? (
              <div className="grid gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
                <div className="grid min-h-44 place-items-center rounded-lg border bg-muted/40">
                  <div className="text-center text-muted-foreground">
                    <GlobeIcon className="mx-auto size-8" strokeWidth={1.25} />
                    <p className="mt-2 max-w-52 truncate text-xs">
                      {primaryHost?.host ?? project.slug}
                    </p>
                  </div>
                </div>
                <div className="grid content-start gap-5 sm:grid-cols-2">
                  <OverviewRow label="Deployment">
                    <Link
                      to={`/projects/${project.id}/deployments/${production.id}`}
                      className="font-mono text-xs font-medium hover:underline"
                    >
                      {production.id.slice(0, 8)}
                    </Link>
                  </OverviewRow>
                  <OverviewRow label="Domains">
                    {primaryHost ? (
                      <span className="flex flex-wrap items-center gap-2">
                        <a
                          href={`http://${primaryHost.host}`}
                          target="_blank"
                          rel="noreferrer"
                          className="truncate font-medium hover:underline"
                        >
                          {primaryHost.host}
                        </a>
                        {productionHosts.length > 1 && (
                          <Link
                            to={`/projects/${project.id}/domains`}
                            className="text-xs text-muted-foreground hover:underline"
                          >
                            +{productionHosts.length - 1} more
                          </Link>
                        )}
                      </span>
                    ) : (
                      <Link
                        to={`/projects/${project.id}/domains`}
                        className="text-muted-foreground hover:underline"
                      >
                        Add a domain
                      </Link>
                    )}
                  </OverviewRow>
                  <OverviewRow label="Status">
                    <span className="flex items-center gap-2">
                      <span
                        className={`size-2 rounded-full ${statusDotClass(production.build_status)}`}
                        aria-hidden="true"
                      />
                      <span className="capitalize">{production.build_status}</span>
                      <ReleaseStatusBadge status={production.release_status} />
                    </span>
                  </OverviewRow>
                  <OverviewRow label="Created">
                    {new Date(production.created_at).toLocaleString()}
                    {production.triggered_by
                      ? ` by ${production.triggered_by.display_name ?? production.triggered_by.email}`
                      : ""}
                  </OverviewRow>
                  <OverviewRow label="Source">
                    <span className="space-y-0.5">
                      <span className="flex items-center gap-1.5">
                        <GitBranchIcon className="size-3.5 text-muted-foreground" />
                        {production.source.branch ?? "—"}
                      </span>
                      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <span className="font-mono">
                          {shortCommit(production.source.commit_hash)}
                        </span>
                        <span className="max-w-56 truncate">
                          {production.source.commit_message ?? ""}
                        </span>
                      </span>
                    </span>
                  </OverviewRow>
                </div>
              </div>
            ) : (
              <Empty className="border-0">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <RocketIcon />
                  </EmptyMedia>
                  <EmptyTitle>No Production Deployment</EmptyTitle>
                  <EmptyDescription>
                    Deploy production from the Deployments page to serve this project on its
                    domains.
                  </EmptyDescription>
                </EmptyHeader>
                <Button asChild variant="outline" size="sm">
                  <Link to={`/projects/${project.id}/deployments`}>Go to Deployments</Link>
                </Button>
              </Empty>
            ))}
        </CardContent>
      </Card>

      <div className="grid gap-4 md:grid-cols-3">
        <Card className="gap-1.5 py-5">
          <CardContent className="space-y-1 px-5">
            <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Domains
            </p>
            <p className="truncate text-sm font-medium">{primaryHost?.host ?? "None assigned"}</p>
            <Link
              to={`/projects/${project.id}/domains`}
              className="text-xs text-muted-foreground hover:underline"
            >
              Manage domains →
            </Link>
          </CardContent>
        </Card>
        <Card className="gap-1.5 py-5">
          <CardContent className="space-y-1 px-5">
            <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Repository
            </p>
            <p className="truncate text-sm font-medium">{project.repository_url ?? "Not set"}</p>
            <p className="text-xs text-muted-foreground">
              Branch {project.default_branch ?? "main"}
            </p>
          </CardContent>
        </Card>
        <Card className="gap-1.5 py-5">
          <CardContent className="space-y-1 px-5">
            <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Latest build
            </p>
            {production ? (
              <BuildStatusBadge status={production.build_status} />
            ) : (
              <p className="text-sm font-medium text-muted-foreground">No builds yet</p>
            )}
            <p className="text-xs text-muted-foreground">
              Runtime <Badge variant="outline">{project.runtime}</Badge>
            </p>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
