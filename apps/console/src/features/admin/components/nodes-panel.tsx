import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  PlayIcon,
  PlusIcon,
  RefreshCwIcon,
  RotateCcwIcon,
  ServerIcon,
  SlidersHorizontalIcon,
  SquareIcon,
  Trash2Icon,
} from "lucide-react";
import { useState } from "react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
  FieldTitle,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { showErrorToast } from "@/lib/toast";

import {
  adminApi,
  type AdminLocalProcessInfo,
  type AdminNode,
  type AdminNodeConfigurationSync,
  type AdminNodeDeletionJob,
  type AdminNodeDeletionPlan,
  type NodeConfiguration,
} from "../admin.api";

function healthBadge(node: AdminNode) {
  if (node.healthy) return <Badge variant="success">Healthy</Badge>;
  if (node.status === "disabled") return <Badge variant="secondary">Disabled</Badge>;
  return <Badge variant="destructive">{node.status}</Badge>;
}

function processBadge(info: AdminLocalProcessInfo) {
  switch (info.process.state) {
    case "running":
      return <Badge variant="success">Running</Badge>;
    case "backoff":
      return <Badge variant="warning">Restarting</Badge>;
    case "failed":
      return <Badge variant="destructive">Failed</Badge>;
    default:
      return <Badge variant="secondary">Stopped</Badge>;
  }
}

function configurationBadge(configuration: AdminNodeConfigurationSync) {
  const revision =
    configuration.status === "applied"
      ? configuration.effective_revision
      : configuration.desired_revision;
  switch (configuration.status) {
    case "pending":
      return <Badge variant="secondary">Pending · r{revision}</Badge>;
    case "applying":
      return <Badge variant="warning">Applying · r{revision}</Badge>;
    case "applied":
      return <Badge variant="success">Applied · r{revision}</Badge>;
    case "failed":
      return <Badge variant="destructive">Failed · r{revision}</Badge>;
  }
}

const ACTIVE_DELETION_STATUSES = new Set(["queued", "migrating", "draining", "deleting"]);

function deletionBadge(deletion: AdminNodeDeletionJob) {
  switch (deletion.status) {
    case "queued":
      return <Badge variant="secondary">Queued for deletion</Badge>;
    case "migrating":
      return <Badge variant="warning">Migrating services</Badge>;
    case "draining":
      return <Badge variant="warning">Draining builds</Badge>;
    case "deleting":
      return <Badge variant="destructive">Deleting</Badge>;
    case "failed":
      return <Badge variant="destructive">Deletion failed</Badge>;
    case "completed":
      return <Badge variant="secondary">Deleted</Badge>;
  }
}

function LocalProcessCard({ info }: { info: AdminLocalProcessInfo }) {
  const queryClient = useQueryClient();

  const actionMutation = useMutation({
    mutationFn: (action: "start" | "stop" | "restart") => adminApi.localNodeProcess(action),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["admin", "nodes"] });
    },
  });

  const running = info.process.state === "running" || info.process.state === "backoff";

  return (
    <div className="rounded-md border p-3 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <ServerIcon className="size-4 text-muted-foreground" />
          <span className="font-medium">Local node process</span>
          {processBadge(info)}
          {info.process.pid != null && (
            <span className="text-xs text-muted-foreground">pid {info.process.pid}</span>
          )}
          {info.process.restart_count > 0 && (
            <span className="text-xs text-muted-foreground">
              {info.process.restart_count} restarts
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">
          {running ? (
            <>
              <Button
                size="sm"
                variant="outline"
                onClick={() => actionMutation.mutate("restart")}
                disabled={actionMutation.isPending}
              >
                <RotateCcwIcon /> Restart
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => actionMutation.mutate("stop")}
                disabled={actionMutation.isPending}
              >
                <SquareIcon /> Stop
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              variant="outline"
              onClick={() => actionMutation.mutate("start")}
              disabled={actionMutation.isPending || !info.managed}
            >
              <PlayIcon /> Start
            </Button>
          )}
        </div>
      </div>
      <p className="mt-1 text-xs text-muted-foreground">
        {info.managed
          ? (info.process.message ??
            "The Control API supervises this grass-node process and restarts it on unexpected exits.")
          : "No generated node config yet. Create a node with “Start local process” to generate one."}
      </p>
    </div>
  );
}

export function NodesPanel() {
  const queryClient = useQueryClient();
  const [revealedToken, setRevealedToken] = useState<{ label: string; token: string } | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [deletingNode, setDeletingNode] = useState<AdminNode | null>(null);
  const [deletionPlan, setDeletionPlan] = useState<AdminNodeDeletionPlan | null>(null);
  const [targetNodeId, setTargetNodeId] = useState("");

  const nodesQuery = useQuery({
    queryKey: ["admin", "nodes"],
    queryFn: adminApi.listNodes,
    refetchInterval: (query) =>
      query.state.data?.nodes.some(
        (node) => node.deletion && ACTIVE_DELETION_STATUSES.has(node.deletion.status),
      )
        ? 2_000
        : 30_000,
  });

  const rotateMutation = useMutation({
    mutationFn: (nodeId: string) => adminApi.rotateNodeToken(nodeId),
    onSuccess: (result) => {
      setRevealedToken({ label: "Rotated node token", token: result.token });
      queryClient.invalidateQueries({ queryKey: ["admin", "nodes"] });
    },
  });

  const planMutation = useMutation({
    mutationFn: (node: AdminNode) => adminApi.nodeDeletionPlan(node.id),
    onSuccess: (plan) => {
      if (plan.requires_target) {
        if (plan.eligible_targets.length === 0) {
          setDeletingNode(null);
          showErrorToast(
            new Error("No healthy Serve Node has enough capacity for this migration."),
            "admin-node-delete-no-target",
          );
          return;
        }
        setDeletionPlan(plan);
        setTargetNodeId("");
        return;
      }
      if (deletingNode) {
        queueMutation.mutate({ node: deletingNode, targetNodeId: null });
      }
    },
    onError: () => {
      setDeletingNode(null);
    },
  });

  const queueMutation = useMutation({
    mutationFn: ({ node, targetNodeId }: { node: AdminNode; targetNodeId: string | null }) =>
      adminApi.queueNodeDeletion(node.id, { target_node_id: targetNodeId }),
    onSuccess: () => {
      setDeletingNode(null);
      setDeletionPlan(null);
      setTargetNodeId("");
      queryClient.invalidateQueries({ queryKey: ["admin", "nodes"] });
    },
    onError: () => {
      setDeletionPlan(null);
      setDeletingNode(null);
      setTargetNodeId("");
    },
  });

  const closeDeletion = () => {
    if (planMutation.isPending || queueMutation.isPending) return;
    setDeletingNode(null);
    setDeletionPlan(null);
    setTargetNodeId("");
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          Nodes build deployments and serve static sites. Heartbeats older than 90 seconds mark a
          node unhealthy.
        </p>
        <CreateNodeDialog
          defaultStartLocal={
            (nodesQuery.data?.local_process?.managed ||
              nodesQuery.data?.local_process?.auto_start) ??
            false
          }
          onCreated={(token, createdWarnings) => {
            setRevealedToken({ label: "New node token", token });
            setWarnings(createdWarnings);
            queryClient.invalidateQueries({ queryKey: ["admin", "nodes"] });
          }}
        />
      </div>

      {nodesQuery.data?.local_process &&
        (nodesQuery.data.local_process.managed || nodesQuery.data.local_process.auto_start) && (
          <LocalProcessCard info={nodesQuery.data.local_process} />
        )}

      {warnings.length > 0 && (
        <div
          role="status"
          className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-sm"
        >
          {warnings.map((warning) => (
            <p key={warning}>{warning}</p>
          ))}
        </div>
      )}

      {revealedToken && (
        <div className="rounded-md border bg-muted/40 p-3 text-sm">
          <p className="font-medium">{revealedToken.label}</p>
          <p className="text-muted-foreground">
            Copy it now — it is shown only once and stored hashed.
          </p>
          <code className="mt-1 block break-all rounded bg-background p-2 text-xs">
            {revealedToken.token}
          </code>
        </div>
      )}
      {nodesQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {nodesQuery.data &&
        (nodesQuery.data.nodes.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <ServerIcon className="mr-1 inline size-4" />
            No nodes yet. Create one to receive a connection token.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Health</TableHead>
                <TableHead>Capabilities</TableHead>
                <TableHead>Serve load</TableHead>
                <TableHead>Configuration</TableHead>
                <TableHead>Version</TableHead>
                <TableHead>Last heartbeat</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {nodesQuery.data.nodes.map((node) => (
                <TableRow key={node.id}>
                  <TableCell>
                    <div className="flex flex-wrap items-center gap-1">
                      <span className="font-medium">{node.name}</span>
                      {node.deletion && deletionBadge(node.deletion)}
                    </div>
                    {node.base_url && (
                      <p className="text-xs text-muted-foreground">{node.base_url}</p>
                    )}
                    {node.deletion &&
                      (node.deletion.total_deployments > 0 || node.deletion.active_builds > 0) && (
                        <p className="text-xs text-muted-foreground">
                          {node.deletion.total_deployments > 0 &&
                            `${node.deletion.migrated_deployments}/${node.deletion.total_deployments} services synced`}
                          {node.deletion.total_deployments > 0 && node.deletion.active_builds > 0
                            ? " · "
                            : ""}
                          {node.deletion.active_builds > 0 &&
                            `${node.deletion.active_builds} active builds`}
                        </p>
                      )}
                    {node.deletion?.error && (
                      <p className="max-w-64 truncate text-xs text-destructive">
                        {node.deletion.error}
                      </p>
                    )}
                  </TableCell>
                  <TableCell>{healthBadge(node)}</TableCell>
                  <TableCell>
                    <div className="flex flex-wrap gap-1">
                      {node.build_enabled && (
                        <Badge variant="outline">Build ×{node.build_concurrency}</Badge>
                      )}
                      {node.serve_enabled && <Badge variant="secondary">Serve</Badge>}
                    </div>
                  </TableCell>
                  <TableCell className="text-xs tabular-nums text-muted-foreground">
                    {node.serve_enabled ? (
                      <div className="flex flex-col gap-0.5 whitespace-nowrap">
                        <p>
                          CPU {node.usage.cpu_millicores}/{node.capacity.cpu_millicores}m
                        </p>
                        <p>
                          Memory {node.usage.memory_mb}/{node.capacity.memory_mb} MB
                        </p>
                        <p>
                          Disk {node.usage.disk_mb}/{node.capacity.disk_mb} MB
                        </p>
                        <p>
                          Deployments {node.usage.deployments}/{node.capacity.max_deployments}
                          {node.overflow_count > 0 ? ` (+${node.overflow_count} overflow)` : ""}
                        </p>
                      </div>
                    ) : (
                      "—"
                    )}
                  </TableCell>
                  <TableCell>
                    <div className="flex flex-col items-start gap-1">
                      {configurationBadge(node.configuration)}
                    </div>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {node.version ?? "—"}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {node.last_heartbeat_at
                      ? new Date(node.last_heartbeat_at).toLocaleString()
                      : "Never"}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex justify-end gap-1">
                      <EditConfigurationDialog node={node} />
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            size="icon"
                            variant="outline"
                            aria-label={`Rotate token for ${node.name}`}
                            onClick={() => rotateMutation.mutate(node.id)}
                            disabled={rotateMutation.isPending}
                          >
                            <RefreshCwIcon />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>Rotate token</TooltipContent>
                      </Tooltip>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            size="icon"
                            variant="outline"
                            aria-label={
                              node.deletion?.status === "failed"
                                ? `Retry deletion for ${node.name}`
                                : `Delete ${node.name}`
                            }
                            onClick={() => setDeletingNode(node)}
                            disabled={
                              planMutation.isPending ||
                              queueMutation.isPending ||
                              (node.deletion != null &&
                                ACTIVE_DELETION_STATUSES.has(node.deletion.status))
                            }
                          >
                            {node.deletion && ACTIVE_DELETION_STATUSES.has(node.deletion.status) ? (
                              <Spinner />
                            ) : node.deletion?.status === "failed" ? (
                              <RotateCcwIcon />
                            ) : (
                              <Trash2Icon />
                            )}
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          {node.deletion && ACTIVE_DELETION_STATUSES.has(node.deletion.status)
                            ? "Deletion in progress"
                            : node.deletion?.status === "failed"
                              ? "Retry deletion"
                              : "Delete node"}
                        </TooltipContent>
                      </Tooltip>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}

      <AlertDialog
        open={deletingNode !== null && deletionPlan === null}
        onOpenChange={(open) => !open && closeDeletion()}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {deletingNode?.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              The node will stop accepting new work. Serve deployments will be copied to a
              replacement before traffic moves, and existing builds will finish before deletion.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={planMutation.isPending}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={planMutation.isPending}
              onClick={(event) => {
                event.preventDefault();
                if (deletingNode) planMutation.mutate(deletingNode);
              }}
            >
              {planMutation.isPending && <Spinner data-icon="inline-start" />}
              Delete node
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <Dialog open={deletionPlan !== null} onOpenChange={(open) => !open && closeDeletion()}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Move services before deleting</DialogTitle>
            <DialogDescription>
              Select one Serve Node with capacity for all {deletionPlan?.assigned_deployments ?? 0}
              assigned services. Traffic stays on {deletingNode?.name} until every copy is ready.
            </DialogDescription>
          </DialogHeader>
          <Field>
            <FieldLabel htmlFor="node-deletion-target">Replacement Serve Node</FieldLabel>
            <Select value={targetNodeId} onValueChange={setTargetNodeId}>
              <SelectTrigger id="node-deletion-target" aria-label="Replacement Serve Node">
                <SelectValue placeholder="Select a Serve Node" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {deletionPlan?.eligible_targets.map((target) => (
                    <SelectItem key={target.id} value={target.id}>
                      {target.name} · {target.available_deployments} deployment slots available
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          <DialogFooter>
            <Button variant="outline" onClick={closeDeletion} disabled={queueMutation.isPending}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              disabled={!targetNodeId || queueMutation.isPending}
              onClick={() =>
                deletingNode &&
                queueMutation.mutate({ node: deletingNode, targetNodeId: targetNodeId || null })
              }
            >
              {queueMutation.isPending && <Spinner data-icon="inline-start" />}
              Queue deletion
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function cloneConfiguration(configuration: NodeConfiguration): NodeConfiguration {
  return JSON.parse(JSON.stringify(configuration)) as NodeConfiguration;
}

function ConfigurationTextField({
  id,
  label,
  value,
  onChange,
  type = "text",
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: "text" | "url";
}) {
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        required
      />
    </Field>
  );
}

function ConfigurationNumberField({
  id,
  label,
  value,
  onChange,
  min = 0,
  max,
}: {
  id: string;
  label: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
}) {
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        step={1}
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
        required
      />
    </Field>
  );
}

function ConfigurationSwitch({
  id,
  label,
  description,
  checked,
  onCheckedChange,
}: {
  id: string;
  label: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        {description && <FieldDescription>{description}</FieldDescription>}
      </FieldContent>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} />
    </Field>
  );
}

function EditConfigurationDialog({ node }: { node: AdminNode }) {
  const queryClient = useQueryClient();
  const source = node.configuration.desired ?? node.configuration.effective;
  const [open, setOpen] = useState(false);
  const [configuration, setConfiguration] = useState<NodeConfiguration | null>(() =>
    source ? cloneConfiguration(source) : null,
  );

  const updateMutation = useMutation({
    mutationFn: (input: NodeConfiguration) => adminApi.updateNodeConfiguration(node.id, input),
    onSuccess: () => {
      setOpen(false);
      queryClient.invalidateQueries({ queryKey: ["admin", "nodes"] });
    },
  });

  const openDialog = () => {
    if (!source) return;
    setConfiguration(cloneConfiguration(source));
    updateMutation.reset();
    if (node.configuration.error) {
      showErrorToast(new Error(node.configuration.error), `admin-node-config-${node.id}`);
    }
    setOpen(true);
  };

  const submit = () => {
    if (!configuration) return;
    if (!configuration.node.capabilities.build && !configuration.node.capabilities.serve) {
      showErrorToast(
        new Error("Enable at least one Node capability."),
        `admin-node-config-validation-${node.id}`,
      );
      return;
    }
    if (configuration.node.capabilities.build && configuration.build.concurrency < 1) {
      showErrorToast(
        new Error("Build concurrency must be positive when Build is enabled."),
        `admin-node-config-validation-${node.id}`,
      );
      return;
    }
    updateMutation.mutate(configuration);
  };

  const updateTarget = (index: number, key: "host" | "ip" | "port", value: string | number) => {
    setConfiguration((current) => {
      if (!current) return current;
      const targets = current.security.private_repository_targets.map((target, targetIndex) =>
        targetIndex === index ? { ...target, [key]: value } : target,
      );
      return { ...current, security: { private_repository_targets: targets } };
    });
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (nextOpen) openDialog();
        else setOpen(false);
      }}
    >
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            size="icon"
            variant="outline"
            aria-label={`Edit configuration for ${node.name}`}
            onClick={openDialog}
            disabled={!source}
          >
            <SlidersHorizontalIcon />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {source ? "Edit configuration" : "Configuration is available after registration"}
        </TooltipContent>
      </Tooltip>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Edit Node configuration</DialogTitle>
          <DialogDescription>
            {node.name} · changes are applied after the Node writes the configuration and restarts.
          </DialogDescription>
        </DialogHeader>
        {configuration && (
          <form
            className="flex flex-col gap-5"
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            <Field orientation="horizontal">
              <FieldContent>
                <FieldTitle>Configuration synchronization</FieldTitle>
                <FieldDescription>
                  Desired r{node.configuration.desired_revision}, effective r
                  {node.configuration.effective_revision}
                </FieldDescription>
              </FieldContent>
              {configurationBadge(node.configuration)}
            </Field>

            <Tabs defaultValue="general">
              <TabsList className="grid h-auto w-full grid-cols-2 sm:grid-cols-4">
                <TabsTrigger value="general">General</TabsTrigger>
                <TabsTrigger value="serve">Serve</TabsTrigger>
                <TabsTrigger value="runtime">Runtime</TabsTrigger>
                <TabsTrigger value="security">Security & logs</TabsTrigger>
              </TabsList>

              <TabsContent value="general" className="mt-3">
                <FieldGroup className="gap-5">
                  <FieldSet className="gap-4">
                    <FieldLegend>Identity</FieldLegend>
                    <div className="grid gap-4 md:grid-cols-2">
                      <ConfigurationTextField
                        id={`node-${node.id}-config-id`}
                        label="Node ID"
                        value={configuration.node.id}
                        onChange={(id) =>
                          setConfiguration((current) =>
                            current ? { ...current, node: { ...current.node, id } } : current,
                          )
                        }
                      />
                      <ConfigurationTextField
                        id={`node-${node.id}-control-api`}
                        label="Control API URL"
                        type="url"
                        value={configuration.node.control_api}
                        onChange={(control_api) =>
                          setConfiguration((current) =>
                            current
                              ? { ...current, node: { ...current.node, control_api } }
                              : current,
                          )
                        }
                      />
                      <ConfigurationTextField
                        id={`node-${node.id}-work-root`}
                        label="Work root"
                        value={configuration.node.work_root}
                        onChange={(work_root) =>
                          setConfiguration((current) =>
                            current
                              ? { ...current, node: { ...current.node, work_root } }
                              : current,
                          )
                        }
                      />
                    </div>
                  </FieldSet>

                  <FieldSet className="gap-3">
                    <FieldLegend>Capabilities</FieldLegend>
                    <FieldGroup className="gap-3">
                      <ConfigurationSwitch
                        id={`node-${node.id}-build-capability`}
                        label="Build"
                        checked={configuration.node.capabilities.build}
                        onCheckedChange={(build) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  node: {
                                    ...current.node,
                                    capabilities: { ...current.node.capabilities, build },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationSwitch
                        id={`node-${node.id}-serve-capability`}
                        label="Serve"
                        checked={configuration.node.capabilities.serve}
                        onCheckedChange={(serve) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  node: {
                                    ...current.node,
                                    capabilities: { ...current.node.capabilities, serve },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                    </FieldGroup>
                  </FieldSet>

                  <FieldSet className="gap-4">
                    <FieldLegend>Build</FieldLegend>
                    <div className="grid gap-4 md:grid-cols-2">
                      <ConfigurationNumberField
                        id={`node-${node.id}-build-concurrency`}
                        label="Build concurrency"
                        min={configuration.node.capabilities.build ? 1 : 0}
                        max={65_535}
                        value={configuration.build.concurrency}
                        onChange={(concurrency) =>
                          setConfiguration((current) =>
                            current
                              ? { ...current, build: { ...current.build, concurrency } }
                              : current,
                          )
                        }
                      />
                      <ConfigurationNumberField
                        id={`node-${node.id}-command-timeout`}
                        label="Command timeout seconds"
                        min={1}
                        value={configuration.build.command_timeout_seconds}
                        onChange={(command_timeout_seconds) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  build: { ...current.build, command_timeout_seconds },
                                }
                              : current,
                          )
                        }
                      />
                    </div>
                    <ConfigurationSwitch
                      id={`node-${node.id}-retain-workspace`}
                      label="Retain workspace on failure"
                      checked={configuration.build.retain_workspace_on_failure}
                      onCheckedChange={(retain_workspace_on_failure) =>
                        setConfiguration((current) =>
                          current
                            ? {
                                ...current,
                                build: { ...current.build, retain_workspace_on_failure },
                              }
                            : current,
                        )
                      }
                    />
                  </FieldSet>
                </FieldGroup>
              </TabsContent>

              <TabsContent value="serve" className="mt-3">
                <FieldGroup className="gap-5">
                  <FieldSet className="gap-4">
                    <FieldLegend>Listener and cache</FieldLegend>
                    <div className="grid gap-4 md:grid-cols-2">
                      <ConfigurationTextField
                        id={`node-${node.id}-serve-host`}
                        label="Serve host"
                        value={configuration.serve.host}
                        onChange={(host) =>
                          setConfiguration((current) =>
                            current ? { ...current, serve: { ...current.serve, host } } : current,
                          )
                        }
                      />
                      <ConfigurationNumberField
                        id={`node-${node.id}-serve-port`}
                        label="Serve port"
                        min={1}
                        max={65_535}
                        value={configuration.serve.port}
                        onChange={(port) =>
                          setConfiguration((current) =>
                            current ? { ...current, serve: { ...current.serve, port } } : current,
                          )
                        }
                      />
                      <ConfigurationTextField
                        id={`node-${node.id}-public-base-url`}
                        label="Public base URL"
                        type="url"
                        value={configuration.serve.public_base_url}
                        onChange={(public_base_url) =>
                          setConfiguration((current) =>
                            current
                              ? { ...current, serve: { ...current.serve, public_base_url } }
                              : current,
                          )
                        }
                      />
                      <ConfigurationNumberField
                        id={`node-${node.id}-metadata-cache-ttl`}
                        label="Metadata cache TTL seconds"
                        value={configuration.serve.metadata_cache_ttl_seconds}
                        onChange={(metadata_cache_ttl_seconds) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  serve: { ...current.serve, metadata_cache_ttl_seconds },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationTextField
                        id={`node-${node.id}-artifact-cache-root`}
                        label="Artifact cache root"
                        value={configuration.serve.artifact_cache_root}
                        onChange={(artifact_cache_root) =>
                          setConfiguration((current) =>
                            current
                              ? { ...current, serve: { ...current.serve, artifact_cache_root } }
                              : current,
                          )
                        }
                      />
                    </div>
                  </FieldSet>

                  <FieldSet className="gap-4">
                    <FieldLegend>Capacity</FieldLegend>
                    <div className="grid gap-4 md:grid-cols-2">
                      <ConfigurationNumberField
                        id={`node-${node.id}-cpu-capacity`}
                        label="CPU millicores"
                        value={configuration.serve.capacity.cpu_millicores}
                        onChange={(cpu_millicores) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  serve: {
                                    ...current.serve,
                                    capacity: { ...current.serve.capacity, cpu_millicores },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationNumberField
                        id={`node-${node.id}-memory-capacity`}
                        label="Memory MB"
                        value={configuration.serve.capacity.memory_mb}
                        onChange={(memory_mb) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  serve: {
                                    ...current.serve,
                                    capacity: { ...current.serve.capacity, memory_mb },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationNumberField
                        id={`node-${node.id}-disk-capacity`}
                        label="Artifact disk MB"
                        value={configuration.serve.capacity.disk_mb}
                        onChange={(disk_mb) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  serve: {
                                    ...current.serve,
                                    capacity: { ...current.serve.capacity, disk_mb },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationNumberField
                        id={`node-${node.id}-max-deployments`}
                        label="Max deployments"
                        min={1}
                        value={configuration.serve.capacity.max_deployments}
                        onChange={(max_deployments) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  serve: {
                                    ...current.serve,
                                    capacity: { ...current.serve.capacity, max_deployments },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                    </div>
                  </FieldSet>

                  <FieldSet className="gap-4">
                    <FieldLegend>SSR lifecycle</FieldLegend>
                    <div className="grid gap-4 md:grid-cols-2">
                      <ConfigurationNumberField
                        id={`node-${node.id}-ssr-idle-stop`}
                        label="Idle stop seconds"
                        value={configuration.serve.ssr.idle_stop_seconds}
                        onChange={(idle_stop_seconds) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  serve: {
                                    ...current.serve,
                                    ssr: { ...current.serve.ssr, idle_stop_seconds },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationNumberField
                        id={`node-${node.id}-ssr-startup-timeout`}
                        label="Startup timeout seconds"
                        value={configuration.serve.ssr.startup_timeout_seconds}
                        onChange={(startup_timeout_seconds) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  serve: {
                                    ...current.serve,
                                    ssr: { ...current.serve.ssr, startup_timeout_seconds },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                    </div>
                  </FieldSet>
                </FieldGroup>
              </TabsContent>

              <TabsContent value="runtime" className="mt-3">
                <FieldGroup className="gap-5">
                  <FieldSet className="gap-4">
                    <FieldLegend>Container runtime</FieldLegend>
                    <div className="grid gap-4 md:grid-cols-2">
                      <Field>
                        <FieldLabel htmlFor={`node-${node.id}-runtime-backend`}>Backend</FieldLabel>
                        <Select
                          value={configuration.runtime.backend}
                          onValueChange={(backend: "docker-socket" | "podman-socket") =>
                            setConfiguration((current) =>
                              current
                                ? { ...current, runtime: { ...current.runtime, backend } }
                                : current,
                            )
                          }
                        >
                          <SelectTrigger id={`node-${node.id}-runtime-backend`} className="w-full">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              <SelectItem value="docker-socket">Docker socket</SelectItem>
                              <SelectItem value="podman-socket">Podman socket</SelectItem>
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      </Field>
                      <ConfigurationTextField
                        id={`node-${node.id}-runtime-socket`}
                        label="Socket"
                        value={configuration.runtime.socket}
                        onChange={(socket) =>
                          setConfiguration((current) =>
                            current
                              ? { ...current, runtime: { ...current.runtime, socket } }
                              : current,
                          )
                        }
                      />
                      <ConfigurationTextField
                        id={`node-${node.id}-build-image`}
                        label="Default build image"
                        value={configuration.runtime.default_build_image}
                        onChange={(default_build_image) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  runtime: { ...current.runtime, default_build_image },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationTextField
                        id={`node-${node.id}-serve-image`}
                        label="Default serve image"
                        value={configuration.runtime.default_serve_image}
                        onChange={(default_serve_image) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  runtime: { ...current.runtime, default_serve_image },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationTextField
                        id={`node-${node.id}-runtime-network`}
                        label="Network"
                        value={configuration.runtime.network}
                        onChange={(network) =>
                          setConfiguration((current) =>
                            current
                              ? { ...current, runtime: { ...current.runtime, network } }
                              : current,
                          )
                        }
                      />
                    </div>
                  </FieldSet>

                  <FieldSet className="gap-4">
                    <FieldLegend>Per-container resources</FieldLegend>
                    <div className="grid gap-4 md:grid-cols-2">
                      <ConfigurationNumberField
                        id={`node-${node.id}-runtime-cpu`}
                        label="CPU limit"
                        min={1}
                        value={configuration.runtime.resources.cpu_limit}
                        onChange={(cpu_limit) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  runtime: {
                                    ...current.runtime,
                                    resources: { ...current.runtime.resources, cpu_limit },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                      <ConfigurationNumberField
                        id={`node-${node.id}-runtime-memory`}
                        label="Memory MB"
                        min={1}
                        value={configuration.runtime.resources.memory_mb}
                        onChange={(memory_mb) =>
                          setConfiguration((current) =>
                            current
                              ? {
                                  ...current,
                                  runtime: {
                                    ...current.runtime,
                                    resources: { ...current.runtime.resources, memory_mb },
                                  },
                                }
                              : current,
                          )
                        }
                      />
                    </div>
                  </FieldSet>
                </FieldGroup>
              </TabsContent>

              <TabsContent value="security" className="mt-3">
                <FieldGroup className="gap-5">
                  <FieldSet className="gap-4">
                    <FieldLegend>Authentication</FieldLegend>
                    <Field orientation="horizontal">
                      <FieldTitle>Node token</FieldTitle>
                      <Badge
                        variant={node.configuration.node_token_configured ? "success" : "secondary"}
                      >
                        {node.configuration.node_token_configured ? "Configured" : "Not configured"}
                      </Badge>
                    </Field>
                  </FieldSet>

                  <FieldSet className="gap-4">
                    <FieldLegend>Private repository targets</FieldLegend>
                    {configuration.security.private_repository_targets.length > 0 && (
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>Host</TableHead>
                            <TableHead>IP address</TableHead>
                            <TableHead>Port</TableHead>
                            <TableHead className="w-12">
                              <span className="sr-only">Actions</span>
                            </TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {configuration.security.private_repository_targets.map(
                            (target, index) => (
                              <TableRow key={`${index}-${target.host}-${target.ip}`}>
                                <TableCell>
                                  <Field>
                                    <FieldLabel
                                      htmlFor={`node-${node.id}-target-${index}-host`}
                                      className="sr-only"
                                    >
                                      Private target host {index + 1}
                                    </FieldLabel>
                                    <Input
                                      id={`node-${node.id}-target-${index}-host`}
                                      value={target.host}
                                      onChange={(event) =>
                                        updateTarget(index, "host", event.target.value)
                                      }
                                      required
                                    />
                                  </Field>
                                </TableCell>
                                <TableCell>
                                  <Field>
                                    <FieldLabel
                                      htmlFor={`node-${node.id}-target-${index}-ip`}
                                      className="sr-only"
                                    >
                                      Private target IP {index + 1}
                                    </FieldLabel>
                                    <Input
                                      id={`node-${node.id}-target-${index}-ip`}
                                      value={target.ip}
                                      onChange={(event) =>
                                        updateTarget(index, "ip", event.target.value)
                                      }
                                      required
                                    />
                                  </Field>
                                </TableCell>
                                <TableCell>
                                  <Field>
                                    <FieldLabel
                                      htmlFor={`node-${node.id}-target-${index}-port`}
                                      className="sr-only"
                                    >
                                      Private target port {index + 1}
                                    </FieldLabel>
                                    <Input
                                      id={`node-${node.id}-target-${index}-port`}
                                      type="number"
                                      min={1}
                                      max={65_535}
                                      step={1}
                                      value={target.port}
                                      onChange={(event) =>
                                        updateTarget(index, "port", Number(event.target.value))
                                      }
                                      required
                                    />
                                  </Field>
                                </TableCell>
                                <TableCell>
                                  <Button
                                    type="button"
                                    size="icon"
                                    variant="ghost"
                                    aria-label={`Remove private target ${index + 1}`}
                                    onClick={() =>
                                      setConfiguration((current) =>
                                        current
                                          ? {
                                              ...current,
                                              security: {
                                                private_repository_targets:
                                                  current.security.private_repository_targets.filter(
                                                    (_, targetIndex) => targetIndex !== index,
                                                  ),
                                              },
                                            }
                                          : current,
                                      )
                                    }
                                  >
                                    <Trash2Icon />
                                  </Button>
                                </TableCell>
                              </TableRow>
                            ),
                          )}
                        </TableBody>
                      </Table>
                    )}
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="self-start"
                      disabled={configuration.security.private_repository_targets.length >= 100}
                      onClick={() =>
                        setConfiguration((current) =>
                          current
                            ? {
                                ...current,
                                security: {
                                  private_repository_targets: [
                                    ...current.security.private_repository_targets,
                                    { host: "", ip: "", port: 443 },
                                  ],
                                },
                              }
                            : current,
                        )
                      }
                    >
                      <PlusIcon data-icon="inline-start" />
                      Add target
                    </Button>
                  </FieldSet>

                  <FieldSet className="gap-4">
                    <FieldLegend>Development and logging</FieldLegend>
                    <ConfigurationSwitch
                      id={`node-${node.id}-verbose-build-log`}
                      label="Verbose build log"
                      checked={configuration.development.verbose_build_log}
                      onCheckedChange={(verbose_build_log) =>
                        setConfiguration((current) =>
                          current ? { ...current, development: { verbose_build_log } } : current,
                        )
                      }
                    />
                    <div className="grid gap-4 md:grid-cols-2">
                      <ConfigurationTextField
                        id={`node-${node.id}-log-filter`}
                        label="Log filter"
                        value={configuration.log.level}
                        onChange={(level) =>
                          setConfiguration((current) =>
                            current ? { ...current, log: { ...current.log, level } } : current,
                          )
                        }
                      />
                      <Field>
                        <FieldLabel htmlFor={`node-${node.id}-log-format`}>Log format</FieldLabel>
                        <Select
                          value={configuration.log.format}
                          onValueChange={(format: "pretty" | "json") =>
                            setConfiguration((current) =>
                              current ? { ...current, log: { ...current.log, format } } : current,
                            )
                          }
                        >
                          <SelectTrigger id={`node-${node.id}-log-format`} className="w-full">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              <SelectItem value="pretty">Pretty</SelectItem>
                              <SelectItem value="json">JSON</SelectItem>
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      </Field>
                    </div>
                  </FieldSet>
                </FieldGroup>
              </TabsContent>
            </Tabs>

            <DialogFooter>
              <Button type="submit" disabled={updateMutation.isPending}>
                {updateMutation.isPending && <Spinner data-icon="inline-start" />}
                {updateMutation.isPending ? "Saving…" : "Save configuration"}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

function CreateNodeDialog({
  defaultStartLocal,
  onCreated,
}: {
  defaultStartLocal: boolean;
  onCreated: (token: string, warnings: string[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [startLocal, setStartLocal] = useState(defaultStartLocal);

  const createMutation = useMutation({
    mutationFn: () => adminApi.createNode({ name, start_local: startLocal }),
    onSuccess: ({ token, warnings }) => {
      setOpen(false);
      setName("");
      onCreated(token, warnings ?? []);
    },
  });

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>
          <PlusIcon /> Add node
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add node</DialogTitle>
          <DialogDescription>
            Creates a node record and returns its connection token once.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim()) createMutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="node-name">Node name</FieldLabel>
            <Input
              id="node-name"
              placeholder="build-node-1"
              value={name}
              onChange={(event) => setName(event.target.value)}
              required
            />
          </Field>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              className="size-4"
              checked={startLocal}
              onChange={(event) => setStartLocal(event.target.checked)}
            />
            Start local process — generate node.toml with this token and run grass-node on this
            machine
          </label>
          <DialogFooter>
            <Button type="submit" disabled={createMutation.isPending}>
              {createMutation.isPending ? "Creating…" : "Create node"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
