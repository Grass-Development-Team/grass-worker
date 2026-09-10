import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlusIcon, Trash2Icon } from "lucide-react";
import { useId, useState } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
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
import { RegionSelect } from "@/features/regions/region-select";
import { adminApi, type AdminRegionalIngress } from "../admin.api";

export function RegionalIngressesPanel() {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ["admin", "regional-ingresses"],
    queryFn: adminApi.listRegionalIngresses,
    refetchInterval: 10_000,
  });
  const invalidate = () => {
    void client.invalidateQueries({ queryKey: ["admin", "regional-ingresses"] });
    void client.invalidateQueries({ queryKey: ["regions"] });
  };
  const remove = useMutation({ mutationFn: adminApi.removeRegionalIngress, onSuccess: invalidate });
  const toggle = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      adminApi.updateRegionalIngress(id, { enabled }),
    onSuccess: invalidate,
  });
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold">Regional entries</h2>
          <p className="text-sm text-muted-foreground">
            One CNAME target per region. Configure its DNS records manually at your DNS provider.
          </p>
        </div>
        <EntryDialog onSaved={invalidate} />
      </div>
      {query.isPending && <Skeleton className="h-40 w-full" />}
      {(query.isError || remove.isError || toggle.isError) && (
        <Alert variant="destructive">
          <AlertDescription>
            {remove.error?.message ??
              toggle.error?.message ??
              "Regional entries could not be loaded."}
          </AlertDescription>
        </Alert>
      )}
      {query.data?.regional_ingresses.length === 0 && (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No regional entries</EmptyTitle>
            <EmptyDescription>
              Create a region in platform settings or Node configuration, then assign its CNAME
              target here.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      )}
      {Boolean(query.data?.regional_ingresses.length) && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Region</TableHead>
              <TableHead>CNAME target</TableHead>
              <TableHead>DNS</TableHead>
              <TableHead>Entry nodes</TableHead>
              <TableHead>Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {query.data?.regional_ingresses.map((entry) => (
              <TableRow key={entry.id}>
                <TableCell>{entry.region}</TableCell>
                <TableCell>
                  <code>{entry.hostname}</code>
                  <p>
                    <Badge variant={entry.enabled ? "success" : "secondary"}>
                      {entry.enabled ? "Enabled" : "Disabled"}
                    </Badge>
                  </p>
                </TableCell>
                <TableCell>
                  <Badge variant={entry.dns_status === "resolved" ? "success" : "warning"}>
                    {entry.dns_status}
                  </Badge>
                  {entry.dns_error && <p className="text-xs text-destructive">{entry.dns_error}</p>}
                  {entry.dns_checked_at && (
                    <p className="text-xs text-muted-foreground">
                      Checked {new Date(entry.dns_checked_at).toLocaleString()}
                    </p>
                  )}
                </TableCell>
                <TableCell>
                  {entry.healthy_nodes.length} ready
                  {entry.node_statuses?.map((node) => (
                    <p key={node.node_id} className="text-xs text-muted-foreground">
                      {node.node_id}: {node.health_status} · HTTPS{" "}
                      {node.tls_ready ? "ready" : "not ready"}
                    </p>
                  ))}
                </TableCell>
                <TableCell>
                  <div className="flex flex-wrap gap-2">
                    <EntryDialog item={entry} onSaved={invalidate} />
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={toggle.isPending}
                      onClick={() => toggle.mutate({ id: entry.id, enabled: !entry.enabled })}
                    >
                      {entry.enabled ? "Disable" : "Enable"}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Remove ${entry.hostname}`}
                      disabled={remove.isPending}
                      onClick={() => remove.mutate(entry.id)}
                    >
                      <Trash2Icon data-icon="inline-start" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  );
}
function EntryDialog({ item, onSaved }: { item?: AdminRegionalIngress; onSaved: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size={item ? "sm" : "default"} variant={item ? "outline" : "default"}>
          {!item && <PlusIcon data-icon="inline-start" />}
          {item ? "Edit" : "Add regional entry"}
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{item ? "Edit regional entry" : "Add regional entry"}</DialogTitle>
          <DialogDescription>
            Customers point their domains to this hostname. Certificates are issued for their
            domains automatically.
          </DialogDescription>
        </DialogHeader>
        {open && (
          <EntryForm
            item={item}
            onSaved={() => {
              setOpen(false);
              onSaved();
            }}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}
function EntryForm({ item, onSaved }: { item?: AdminRegionalIngress; onSaved: () => void }) {
  const id = useId();
  const [region, setRegion] = useState(item?.region ?? "");
  const [hostname, setHostname] = useState(item?.hostname ?? "");
  const [path, setPath] = useState(item?.health_check_path ?? "/_grass/health");
  const [interval, setInterval] = useState(item?.health_check_interval_seconds ?? 30);
  const mutation = useMutation({
    mutationFn: () => {
      const input = {
        hostname: hostname.trim(),
        health_check_path: path,
        health_check_interval_seconds: interval,
      };
      return item
        ? adminApi.updateRegionalIngress(item.id, input)
        : adminApi.createRegionalIngress({ ...input, region });
    },
    onSuccess: onSaved,
  });
  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (region && hostname.trim()) mutation.mutate();
      }}
    >
      <FieldGroup>
        <Field>
          <FieldLabel htmlFor={id + "-region"}>Region</FieldLabel>
          <RegionSelect
            id={id + "-region"}
            value={region}
            onChange={setRegion}
            unusedOnly={!item}
            disabled={!!item}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={id + "-hostname"}>CNAME target</FieldLabel>
          <Input
            id={id + "-hostname"}
            value={hostname}
            onChange={(e) => setHostname(e.target.value)}
            placeholder="hk.entry.example.com"
            required
          />
          <FieldDescription>
            Manually point this hostname to the regional entry IPs. Use DNS-only records when using
            Cloudflare.
          </FieldDescription>
        </Field>
      </FieldGroup>
      <details>
        <summary className="cursor-pointer text-sm">Advanced health checks</summary>
        <FieldGroup className="mt-4">
          <Field>
            <FieldLabel htmlFor={id + "-path"}>Health check path</FieldLabel>
            <Input
              id={id + "-path"}
              value={path}
              onChange={(e) => setPath(e.target.value)}
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor={id + "-interval"}>Interval (seconds)</FieldLabel>
            <Input
              id={id + "-interval"}
              type="number"
              min={5}
              max={3600}
              value={interval}
              onChange={(e) => setInterval(Number(e.target.value))}
              required
            />
          </Field>
        </FieldGroup>
      </details>
      {mutation.isError && (
        <Alert variant="destructive">
          <AlertDescription>{mutation.error.message}</AlertDescription>
        </Alert>
      )}
      <DialogFooter>
        <Button type="submit" disabled={mutation.isPending || !region || !hostname.trim()}>
          {mutation.isPending ? "Saving…" : item ? "Save changes" : "Create entry"}
        </Button>
      </DialogFooter>
    </form>
  );
}
