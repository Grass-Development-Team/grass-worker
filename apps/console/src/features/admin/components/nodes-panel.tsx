import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  PlayIcon,
  PlusIcon,
  RefreshCwIcon,
  RotateCcwIcon,
  ServerIcon,
  SquareIcon,
} from "lucide-react";
import { useState } from "react";

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

import { adminApi, type AdminLocalProcessInfo, type AdminNode } from "../admin.api";

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

function LocalProcessCard({ info }: { info: AdminLocalProcessInfo }) {
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);

  const actionMutation = useMutation({
    mutationFn: (action: "start" | "stop" | "restart") => adminApi.localNodeProcess(action),
    onSuccess: () => {
      setError(null);
      queryClient.invalidateQueries({ queryKey: ["admin", "nodes"] });
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to control the local process."),
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
      {error && (
        <p role="alert" className="mt-1 text-xs text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}

export function NodesPanel() {
  const queryClient = useQueryClient();
  const [revealedToken, setRevealedToken] = useState<{ label: string; token: string } | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  const nodesQuery = useQuery({
    queryKey: ["admin", "nodes"],
    queryFn: adminApi.listNodes,
    refetchInterval: 30_000,
  });

  const rotateMutation = useMutation({
    mutationFn: (nodeId: string) => adminApi.rotateNodeToken(nodeId),
    onSuccess: (result) => {
      setRevealedToken({ label: "Rotated node token", token: result.token });
      setError(null);
      queryClient.invalidateQueries({ queryKey: ["admin", "nodes"] });
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to rotate the token."),
  });

  return (
    <div className="space-y-4">
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
          role="alert"
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
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      {nodesQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {nodesQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {nodesQuery.error instanceof Error ? nodesQuery.error.message : "Unable to load nodes."}
        </p>
      )}
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
                <TableHead>Version</TableHead>
                <TableHead>Last heartbeat</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {nodesQuery.data.nodes.map((node) => (
                <TableRow key={node.id}>
                  <TableCell>
                    <span className="font-medium">{node.name}</span>
                    {node.base_url && (
                      <p className="text-xs text-muted-foreground">{node.base_url}</p>
                    )}
                  </TableCell>
                  <TableCell>{healthBadge(node)}</TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    build ×{node.build_concurrency} · serve
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
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => rotateMutation.mutate(node.id)}
                      disabled={rotateMutation.isPending}
                    >
                      <RefreshCwIcon /> Rotate token
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}
    </div>
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
          {createMutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {createMutation.error instanceof Error
                ? createMutation.error.message
                : "Unable to create the node."}
            </p>
          )}
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
