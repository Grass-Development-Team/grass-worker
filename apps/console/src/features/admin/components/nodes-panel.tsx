import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlusIcon, RefreshCwIcon, ServerIcon } from "lucide-react";
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

import { adminApi, type AdminNode } from "../admin.api";

function healthBadge(node: AdminNode) {
  if (node.healthy) return <Badge variant="success">Healthy</Badge>;
  if (node.status === "disabled") return <Badge variant="secondary">Disabled</Badge>;
  return <Badge variant="destructive">{node.status}</Badge>;
}

export function NodesPanel() {
  const queryClient = useQueryClient();
  const [revealedToken, setRevealedToken] = useState<{ label: string; token: string } | null>(null);
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
          onCreated={(token) => {
            setRevealedToken({ label: "New node token", token });
            queryClient.invalidateQueries({ queryKey: ["admin", "nodes"] });
          }}
        />
      </div>

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

function CreateNodeDialog({ onCreated }: { onCreated: (token: string) => void }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");

  const createMutation = useMutation({
    mutationFn: () => adminApi.createNode({ name }),
    onSuccess: ({ token }) => {
      setOpen(false);
      setName("");
      onCreated(token);
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
