import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { GlobeIcon, PlusIcon, Trash2Icon } from "lucide-react";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { adminApi, type HostSourceKind } from "../admin.api";

export function HostSourcesPanel() {
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);

  const sourcesQuery = useQuery({
    queryKey: ["admin", "host-sources"],
    queryFn: adminApi.listHostSources,
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "host-sources"] });

  const removeMutation = useMutation({
    mutationFn: (sourceId: string) => adminApi.removeHostSource(sourceId),
    onSuccess: invalidate,
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to remove the host source."),
  });
  const toggleDefaultMutation = useMutation({
    mutationFn: ({ sourceId, isDefault }: { sourceId: string; isDefault: boolean }) =>
      adminApi.updateHostSource(sourceId, { is_default: isDefault }),
    onSuccess: invalidate,
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to update the host source."),
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          Host sources provide platform domains. Wildcard sources activate immediately; DNS provider
          sources are a configurable placeholder in the first stage.
        </p>
        <CreateHostSourceDialog
          onCreated={() => {
            setError(null);
            invalidate();
          }}
        />
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      {sourcesQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {sourcesQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {sourcesQuery.error instanceof Error
            ? sourcesQuery.error.message
            : "Unable to load host sources."}
        </p>
      )}
      {sourcesQuery.data &&
        (sourcesQuery.data.sources.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <GlobeIcon className="mr-1 inline size-4" />
            No host sources yet. Add one so projects can receive platform domains.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Label</TableHead>
                <TableHead>Kind</TableHead>
                <TableHead>Base domain</TableHead>
                <TableHead>Flags</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {sourcesQuery.data.sources.map((source) => (
                <TableRow key={source.id}>
                  <TableCell className="font-medium">{source.label}</TableCell>
                  <TableCell>
                    <Badge variant="outline">{source.kind.replace("_", " ")}</Badge>
                  </TableCell>
                  <TableCell className="font-mono text-sm">{source.base_domain}</TableCell>
                  <TableCell className="space-x-1">
                    {source.is_default && <Badge variant="success">Default</Badge>}
                    {source.allows_auto_assign && <Badge variant="outline">Auto-assign</Badge>}
                    {!source.enabled && <Badge variant="secondary">Disabled</Badge>}
                  </TableCell>
                  <TableCell className="space-x-1 text-right">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() =>
                        toggleDefaultMutation.mutate({
                          sourceId: source.id,
                          isDefault: !source.is_default,
                        })
                      }
                      disabled={toggleDefaultMutation.isPending}
                    >
                      {source.is_default ? "Unset default" : "Make default"}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Remove ${source.label}`}
                      onClick={() => removeMutation.mutate(source.id)}
                      disabled={removeMutation.isPending}
                    >
                      <Trash2Icon />
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

function CreateHostSourceDialog({ onCreated }: { onCreated: () => void }) {
  const [open, setOpen] = useState(false);
  const [label, setLabel] = useState("");
  const [kind, setKind] = useState<HostSourceKind>("wildcard");
  const [baseDomain, setBaseDomain] = useState("");
  const [isDefault, setIsDefault] = useState(false);

  const createMutation = useMutation({
    mutationFn: () =>
      adminApi.createHostSource({
        label,
        kind,
        base_domain: baseDomain,
        is_default: isDefault,
      }),
    onSuccess: () => {
      setOpen(false);
      setLabel("");
      setBaseDomain("");
      setIsDefault(false);
      onCreated();
    },
  });

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>
          <PlusIcon /> Add host source
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add host source</DialogTitle>
          <DialogDescription>
            Wildcard sources expect *.base-domain DNS pointing at the node serve listener.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (label.trim() && baseDomain.trim()) createMutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="source-label">Label</FieldLabel>
            <Input
              id="source-label"
              value={label}
              onChange={(event) => setLabel(event.target.value)}
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="source-kind">Kind</FieldLabel>
            <Select value={kind} onValueChange={(value) => setKind(value as HostSourceKind)}>
              <SelectTrigger id="source-kind">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="wildcard">Wildcard DNS</SelectItem>
                <SelectItem value="dns_provider">DNS provider (placeholder)</SelectItem>
                <SelectItem value="manual">Manual</SelectItem>
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor="source-domain">Base domain</FieldLabel>
            <Input
              id="source-domain"
              placeholder="apps.example.com"
              value={baseDomain}
              onChange={(event) => setBaseDomain(event.target.value)}
              required
            />
          </Field>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={isDefault}
              onChange={(event) => setIsDefault(event.target.checked)}
            />
            Use as the default source for automatic assignment
          </label>
          {createMutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {createMutation.error instanceof Error
                ? createMutation.error.message
                : "Unable to create the host source."}
            </p>
          )}
          <DialogFooter>
            <Button type="submit" disabled={createMutation.isPending}>
              {createMutation.isPending ? "Creating…" : "Create source"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
