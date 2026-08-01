import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeftIcon,
  CheckCircle2Icon,
  CircleOffIcon,
  Globe2Icon,
  HistoryIcon,
  RefreshCwIcon,
  RocketIcon,
  SaveIcon,
  Trash2Icon,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useParams } from "react-router";

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
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import {
  BuildStatusBadge,
  ReleaseStatusBadge,
  ServeStatusBadge,
} from "@/features/deployments/components/status-badges";

import { adminApi, type AdminGovernanceDeployment, type AdminProjectDomain } from "../admin.api";

type GovernanceAction =
  | { kind: "withdraw"; deployment: AdminGovernanceDeployment }
  | { kind: "republish"; deployment: AdminGovernanceDeployment }
  | { kind: "approve-domain"; domain: AdminProjectDomain }
  | { kind: "reject-domain"; domain: AdminProjectDomain }
  | { kind: "delete-domain"; domain: AdminProjectDomain };

function optionalReason(reason: string): string | undefined {
  return reason.trim() || undefined;
}

function errorMessage(cause: unknown, fallback: string): string {
  return cause instanceof Error ? cause.message : fallback;
}

function domainStatusBadge(status: AdminProjectDomain["status"]) {
  switch (status) {
    case "active":
      return <Badge variant="success">Active</Badge>;
    case "failed":
      return <Badge variant="destructive">Failed</Badge>;
    case "disabled":
      return <Badge variant="secondary">Disabled</Badge>;
    default:
      return <Badge variant="warning">Pending</Badge>;
  }
}

function domainReviewBadge(status: AdminProjectDomain["review_status"]) {
  switch (status) {
    case "approved":
      return <Badge variant="success">Approved</Badge>;
    case "rejected":
      return <Badge variant="destructive">Rejected</Badge>;
    case "pending":
      return <Badge variant="warning">Pending review</Badge>;
    default:
      return <Badge variant="outline">Not required</Badge>;
  }
}

function actionCopy(action: GovernanceAction) {
  switch (action.kind) {
    case "withdraw":
      return {
        title: "Withdraw deployment",
        description:
          "This immediately stops public access, retains the deployment, and invalidates its approval. Republishing may require review again.",
        confirm: "Confirm withdrawal",
        destructive: true,
      };
    case "republish":
      return {
        title: "Republish deployment",
        description:
          "The deployment will return to review or publication according to the team's effective release policy.",
        confirm: "Confirm republish",
        destructive: false,
      };
    case "approve-domain":
      return {
        title: "Approve domain",
        description: `Approve ${action.domain.host} for this project.`,
        confirm: "Confirm approval",
        destructive: false,
      };
    case "reject-domain":
      return {
        title: "Reject domain",
        description: `Reject ${action.domain.host} and stop it from serving this project.`,
        confirm: "Confirm rejection",
        destructive: true,
      };
    case "delete-domain":
      return {
        title: "Delete domain",
        description: `Remove ${action.domain.host} from this project. This does not delete the project.`,
        confirm: "Confirm deletion",
        destructive: true,
      };
  }
}

function LoadingTable() {
  return <Skeleton className="h-48 w-full" aria-busy="true" />;
}

function QueryError({ error, fallback }: { error: unknown; fallback: string }) {
  return (
    <p role="alert" className="text-sm text-destructive">
      {errorMessage(error, fallback)}
    </p>
  );
}

export function ProjectGovernancePage() {
  const { projectId = "" } = useParams<{ projectId: string }>();
  const queryClient = useQueryClient();
  const [tab, setTab] = useState("overview");
  const [slug, setSlug] = useState("");
  const [slugReason, setSlugReason] = useState("");
  const [activityPage, setActivityPage] = useState(1);
  const [action, setAction] = useState<GovernanceAction | null>(null);
  const [actionReason, setActionReason] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const projectQuery = useQuery({
    queryKey: ["admin", "project", projectId],
    queryFn: () => adminApi.getProject(projectId),
    enabled: Boolean(projectId),
  });
  const deploymentsQuery = useQuery({
    queryKey: ["admin", "project", projectId, "deployments"],
    queryFn: () => adminApi.listProjectDeployments(projectId),
    enabled: Boolean(projectId) && tab === "deployments",
  });
  const domainsQuery = useQuery({
    queryKey: ["admin", "project", projectId, "domains"],
    queryFn: () => adminApi.listProjectDomains(projectId),
    enabled: Boolean(projectId) && tab === "domains",
  });
  const activityQuery = useQuery({
    queryKey: ["admin", "project", projectId, "activity", activityPage],
    queryFn: () => adminApi.listProjectActivity(projectId, activityPage),
    enabled: Boolean(projectId) && tab === "activity",
  });

  useEffect(() => {
    if (projectQuery.data) setSlug(projectQuery.data.project.slug);
  }, [projectQuery.data]);

  const slugMutation = useMutation({
    mutationFn: () =>
      adminApi.updateProjectSlug(projectId, slug.trim(), optionalReason(slugReason)),
    onSuccess: async () => {
      setError(null);
      setMessage("Public slug updated.");
      setSlugReason("");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["admin", "project", projectId] }),
        queryClient.invalidateQueries({ queryKey: ["admin", "projects"] }),
      ]);
    },
    onError: (cause) => {
      setMessage(null);
      setError(errorMessage(cause, "Unable to update the public slug."));
    },
  });

  const governanceMutation = useMutation({
    mutationFn: async ({ selected, reason }: { selected: GovernanceAction; reason?: string }) => {
      switch (selected.kind) {
        case "withdraw":
          return adminApi.withdrawDeployment(selected.deployment.id, reason);
        case "republish":
          return adminApi.republishDeployment(selected.deployment.id, reason);
        case "approve-domain":
          return adminApi.approveDomain(selected.domain.id, reason);
        case "reject-domain":
          return adminApi.rejectDomain(selected.domain.id, reason);
        case "delete-domain":
          return adminApi.deleteDomain(selected.domain.id, reason);
      }
    },
    onSuccess: async (_result, variables) => {
      const isDeployment =
        variables.selected.kind === "withdraw" || variables.selected.kind === "republish";
      setError(null);
      setMessage("Governance action completed.");
      setAction(null);
      setActionReason("");
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: ["admin", "project", projectId, isDeployment ? "deployments" : "domains"],
        }),
        queryClient.invalidateQueries({
          queryKey: ["admin", "project", projectId, "activity"],
        }),
        queryClient.invalidateQueries({ queryKey: ["admin", "projects"] }),
      ]);
    },
    onError: (cause) => {
      setMessage(null);
      setError(errorMessage(cause, "Unable to complete the governance action."));
    },
  });

  if (projectQuery.isLoading) {
    return <Skeleton className="h-96 w-full" aria-busy="true" />;
  }

  if (projectQuery.isError) {
    return <QueryError error={projectQuery.error} fallback="Unable to load the project." />;
  }

  if (!projectQuery.data) return null;

  const { project, team } = projectQuery.data;
  const dialog = action ? actionCopy(action) : null;
  const overviewDetails = [
    ["Project UUID", project.uuid],
    ["Project name", project.name],
    ["Team", team ? `${team.name} (${team.slug})` : "Not available"],
    ["Runtime", project.runtime],
    ["Repository", project.repository_url ?? "Not configured"],
    ["Default branch", project.default_branch ?? "Not configured"],
    ["Install command", project.install_command ?? "Not configured"],
    ["Build command", project.build_command ?? "Not configured"],
    ["Output directory", project.output_directory ?? "Not configured"],
    ["Archived", project.archived_at ? new Date(project.archived_at).toLocaleString() : "No"],
    ["Created", new Date(project.created_at).toLocaleString()],
    ["Updated", new Date(project.updated_at).toLocaleString()],
  ];

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-3">
        <Button asChild variant="ghost" size="sm" className="w-fit">
          <Link to="/admin/projects">
            <ArrowLeftIcon data-icon="inline-start" />
            All projects
          </Link>
        </Button>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 className="truncate text-xl font-semibold">{project.name}</h2>
            <p className="text-sm text-muted-foreground">
              Platform governance for project configuration, releases, domains, and activity.
            </p>
          </div>
          {project.archived_at && <Badge variant="secondary">Archived</Badge>}
        </div>
      </div>

      {message && (
        <p role="status" className="text-sm text-muted-foreground">
          {message}
        </p>
      )}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      <Tabs value={tab} onValueChange={setTab}>
        <TabsList className="max-w-full overflow-x-auto">
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="deployments">Deployments</TabsTrigger>
          <TabsTrigger value="domains">Domains</TabsTrigger>
          <TabsTrigger value="activity">Activity</TabsTrigger>
        </TabsList>

        <TabsContent value="overview" className="flex flex-col gap-8 pt-4">
          <section className="flex flex-col gap-4">
            <div>
              <h3 className="text-sm font-semibold">Public identifier</h3>
              <p className="text-sm text-muted-foreground">
                Change the public slug used to identify this project.
              </p>
            </div>
            <form
              className="max-w-xl"
              onSubmit={(event) => {
                event.preventDefault();
                setMessage(null);
                setError(null);
                slugMutation.mutate();
              }}
            >
              <FieldGroup className="gap-4">
                <Field>
                  <FieldLabel htmlFor="project-public-slug">Public slug</FieldLabel>
                  <Input
                    id="project-public-slug"
                    value={slug}
                    onChange={(event) => setSlug(event.target.value)}
                    required
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="project-slug-reason">Reason (optional)</FieldLabel>
                  <Textarea
                    id="project-slug-reason"
                    value={slugReason}
                    onChange={(event) => setSlugReason(event.target.value)}
                    rows={3}
                  />
                </Field>
                <Button
                  type="submit"
                  className="w-fit"
                  disabled={slugMutation.isPending || !slug.trim() || slug.trim() === project.slug}
                >
                  <SaveIcon data-icon="inline-start" />
                  {slugMutation.isPending ? "Saving..." : "Save slug"}
                </Button>
              </FieldGroup>
            </form>
          </section>

          <section className="flex flex-col gap-4 border-y py-5">
            <div>
              <h3 className="text-sm font-semibold">Project configuration</h3>
              <p className="text-sm text-muted-foreground">
                Platform administrators can inspect these values but cannot edit them here.
              </p>
            </div>
            <dl className="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-[10rem_minmax(0,1fr)]">
              {overviewDetails.map(([label, value]) => (
                <div key={label} className="contents">
                  <dt className="text-muted-foreground">{label}</dt>
                  <dd className="min-w-0 break-words font-mono text-xs">{value}</dd>
                </div>
              ))}
            </dl>
            <div className="grid gap-4 lg:grid-cols-2">
              <div className="flex min-w-0 flex-col gap-2">
                <h4 className="text-sm font-medium">Source configuration</h4>
                <pre className="overflow-x-auto rounded-md bg-muted p-3 font-mono text-xs">
                  {JSON.stringify(project.source_config, null, 2)}
                </pre>
              </div>
              <div className="flex min-w-0 flex-col gap-2">
                <h4 className="text-sm font-medium">Build configuration</h4>
                <pre className="overflow-x-auto rounded-md bg-muted p-3 font-mono text-xs">
                  {JSON.stringify(project.build_config, null, 2)}
                </pre>
              </div>
            </div>
          </section>
        </TabsContent>

        <TabsContent value="deployments" className="pt-4">
          {deploymentsQuery.isLoading && <LoadingTable />}
          {deploymentsQuery.isError && (
            <QueryError
              error={deploymentsQuery.error}
              fallback="Unable to load project deployments."
            />
          )}
          {deploymentsQuery.data &&
            (deploymentsQuery.data.deployments.length === 0 ? (
              <Empty>
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <RocketIcon />
                  </EmptyMedia>
                  <EmptyTitle>No deployments</EmptyTitle>
                  <EmptyDescription>This project has no retained deployments.</EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Deployment</TableHead>
                    <TableHead>Environment</TableHead>
                    <TableHead>Build</TableHead>
                    <TableHead>Serve</TableHead>
                    <TableHead>Release</TableHead>
                    <TableHead>Created</TableHead>
                    <TableHead className="text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {deploymentsQuery.data.deployments.map((deployment) => (
                    <TableRow key={deployment.id}>
                      <TableCell>
                        <p className="font-mono text-xs">{deployment.id}</p>
                        <p className="max-w-56 truncate text-xs text-muted-foreground">
                          {deployment.commit_message ??
                            deployment.source_branch ??
                            "No source details"}
                        </p>
                      </TableCell>
                      <TableCell className="capitalize">{deployment.environment}</TableCell>
                      <TableCell>
                        <BuildStatusBadge status={deployment.build_status} />
                      </TableCell>
                      <TableCell>
                        <ServeStatusBadge status={deployment.serve_status} />
                      </TableCell>
                      <TableCell>
                        <ReleaseStatusBadge status={deployment.release_status} />
                      </TableCell>
                      <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                        {new Date(deployment.created_at).toLocaleString()}
                      </TableCell>
                      <TableCell>
                        <div className="flex justify-end gap-1">
                          {deployment.release_status === "active" && (
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              onClick={() => setAction({ kind: "withdraw", deployment })}
                            >
                              <CircleOffIcon data-icon="inline-start" />
                              Withdraw deployment
                            </Button>
                          )}
                          {["draft", "approved", "rejected"].includes(deployment.release_status) &&
                            deployment.build_status === "ready" &&
                            ["ready", "retired"].includes(deployment.serve_status) && (
                              <Button
                                type="button"
                                size="sm"
                                variant="outline"
                                onClick={() => setAction({ kind: "republish", deployment })}
                              >
                                <RefreshCwIcon data-icon="inline-start" />
                                Republish deployment
                              </Button>
                            )}
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ))}
        </TabsContent>

        <TabsContent value="domains" className="pt-4">
          {domainsQuery.isLoading && <LoadingTable />}
          {domainsQuery.isError && (
            <QueryError error={domainsQuery.error} fallback="Unable to load project domains." />
          )}
          {domainsQuery.data &&
            (domainsQuery.data.domains.length === 0 ? (
              <Empty>
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <Globe2Icon />
                  </EmptyMedia>
                  <EmptyTitle>No domains</EmptyTitle>
                  <EmptyDescription>This project has no existing domain bindings.</EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Domain</TableHead>
                    <TableHead>Kind</TableHead>
                    <TableHead>Environment</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead>Review</TableHead>
                    <TableHead className="text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {domainsQuery.data.domains.map((domain) => (
                    <TableRow key={domain.id}>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <span className="font-medium">{domain.host}</span>
                          {domain.is_primary && <Badge variant="outline">Primary</Badge>}
                        </div>
                        {domain.failure_reason && (
                          <p className="text-xs text-destructive">{domain.failure_reason}</p>
                        )}
                      </TableCell>
                      <TableCell className="capitalize">{domain.kind}</TableCell>
                      <TableCell className="capitalize">{domain.environment}</TableCell>
                      <TableCell>{domainStatusBadge(domain.status)}</TableCell>
                      <TableCell>{domainReviewBadge(domain.review_status)}</TableCell>
                      <TableCell>
                        <div className="flex flex-wrap justify-end gap-1">
                          {domain.kind === "custom" && domain.review_status !== "approved" && (
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              onClick={() => setAction({ kind: "approve-domain", domain })}
                            >
                              <CheckCircle2Icon data-icon="inline-start" />
                              Approve domain
                            </Button>
                          )}
                          {domain.kind === "custom" && domain.review_status !== "rejected" && (
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              onClick={() => setAction({ kind: "reject-domain", domain })}
                            >
                              <CircleOffIcon data-icon="inline-start" />
                              Reject domain
                            </Button>
                          )}
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            onClick={() => setAction({ kind: "delete-domain", domain })}
                          >
                            <Trash2Icon data-icon="inline-start" />
                            Delete domain
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ))}
        </TabsContent>

        <TabsContent value="activity" className="flex flex-col gap-4 pt-4">
          {activityQuery.isLoading && <LoadingTable />}
          {activityQuery.isError && (
            <QueryError error={activityQuery.error} fallback="Unable to load project activity." />
          )}
          {activityQuery.data &&
            (activityQuery.data.events.length === 0 ? (
              <Empty>
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <HistoryIcon />
                  </EmptyMedia>
                  <EmptyTitle>No activity</EmptyTitle>
                  <EmptyDescription>No governance or project events were found.</EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Time</TableHead>
                    <TableHead>Action</TableHead>
                    <TableHead>Target</TableHead>
                    <TableHead>Result</TableHead>
                    <TableHead>Reason</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {activityQuery.data.events.map((event) => (
                    <TableRow key={event.id}>
                      <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                        {new Date(event.created_at).toLocaleString()}
                      </TableCell>
                      <TableCell className="font-mono text-xs">{event.action}</TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {event.target_type}
                        {event.target_id ? ` / ${event.target_id}` : ""}
                      </TableCell>
                      <TableCell>
                        <Badge
                          variant={
                            event.result === "success"
                              ? "success"
                              : event.result === "denied"
                                ? "warning"
                                : "destructive"
                          }
                        >
                          {event.result}
                        </Badge>
                      </TableCell>
                      <TableCell className="max-w-72 text-sm text-muted-foreground">
                        {event.reason ?? "None"}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ))}
          {activityQuery.data && activityQuery.data.pagination.total_pages > 1 && (
            <div className="flex items-center justify-end gap-2">
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={activityPage <= 1}
                onClick={() => setActivityPage((page) => Math.max(1, page - 1))}
              >
                Previous
              </Button>
              <span className="text-xs text-muted-foreground">
                Page {activityQuery.data.pagination.page} of{" "}
                {activityQuery.data.pagination.total_pages}
              </span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={activityPage >= activityQuery.data.pagination.total_pages}
                onClick={() => setActivityPage((page) => page + 1)}
              >
                Next
              </Button>
            </div>
          )}
        </TabsContent>
      </Tabs>

      <Dialog
        open={action !== null}
        onOpenChange={(open) => {
          if (!open && !governanceMutation.isPending) {
            setAction(null);
            setActionReason("");
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{dialog?.title}</DialogTitle>
            <DialogDescription>{dialog?.description}</DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="governance-reason">Reason (optional)</FieldLabel>
              <Textarea
                id="governance-reason"
                value={actionReason}
                onChange={(event) => setActionReason(event.target.value)}
                rows={4}
              />
            </Field>
          </FieldGroup>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={governanceMutation.isPending}
              onClick={() => {
                setAction(null);
                setActionReason("");
              }}
            >
              Cancel
            </Button>
            <Button
              type="button"
              variant={dialog?.destructive ? "destructive" : "default"}
              disabled={!action || governanceMutation.isPending}
              onClick={() => {
                if (!action) return;
                setMessage(null);
                setError(null);
                governanceMutation.mutate({
                  selected: action,
                  reason: optionalReason(actionReason),
                });
              }}
            >
              {governanceMutation.isPending ? "Working..." : dialog?.confirm}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
