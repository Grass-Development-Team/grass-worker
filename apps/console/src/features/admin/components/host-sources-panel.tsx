import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { GlobeIcon, PencilIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
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

import { adminApi, type AdminHostSource, type HostSourceKind } from "../admin.api";

type CloudflareRecordType = "A" | "AAAA" | "CNAME";

interface CloudflareFormState {
  apiToken: string;
  zoneId: string;
  recordType: CloudflareRecordType;
  recordValue: string;
  proxied: boolean;
}

const emptyCloudflareForm: CloudflareFormState = {
  apiToken: "",
  zoneId: "",
  recordType: "A",
  recordValue: "",
  proxied: false,
};

/** Builds the config patch; blank fields are omitted so stored values stay. */
function cloudflareConfig(
  form: CloudflareFormState,
  { includeShape }: { includeShape: boolean },
): Record<string, unknown> {
  const config: Record<string, unknown> = {};
  if (form.apiToken.trim()) config.api_token = form.apiToken.trim();
  if (form.zoneId.trim()) config.zone_id = form.zoneId.trim();
  if (form.recordValue.trim()) config.record_value = form.recordValue.trim();
  if (includeShape) {
    config.record_type = form.recordType;
    config.proxied = form.proxied;
  }
  return config;
}

function CloudflareFields({
  form,
  onChange,
  requireSecrets,
  idPrefix,
}: {
  form: CloudflareFormState;
  onChange: (next: CloudflareFormState) => void;
  requireSecrets: boolean;
  idPrefix: string;
}) {
  return (
    <div className="space-y-4 rounded-md border bg-muted/30 p-4">
      <Field>
        <FieldLabel htmlFor={`${idPrefix}-api-token`}>API token</FieldLabel>
        <Input
          id={`${idPrefix}-api-token`}
          type="password"
          autoComplete="off"
          placeholder={requireSecrets ? "" : "Leave blank to keep the stored token"}
          value={form.apiToken}
          onChange={(event) => onChange({ ...form, apiToken: event.target.value })}
          required={requireSecrets}
        />
        <FieldDescription>
          Needs the Zone / DNS / Edit permission. Stored server-side, never shown again.
        </FieldDescription>
      </Field>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field>
          <FieldLabel htmlFor={`${idPrefix}-zone-id`}>Zone ID</FieldLabel>
          <Input
            id={`${idPrefix}-zone-id`}
            placeholder={requireSecrets ? "" : "Leave blank to keep"}
            value={form.zoneId}
            onChange={(event) => onChange({ ...form, zoneId: event.target.value })}
            required={requireSecrets}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${idPrefix}-record-type`}>Record type</FieldLabel>
          <Select
            value={form.recordType}
            onValueChange={(value) => onChange({ ...form, recordType: value as never })}
          >
            <SelectTrigger id={`${idPrefix}-record-type`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="A">A — node IPv4</SelectItem>
              <SelectItem value="AAAA">AAAA — node IPv6</SelectItem>
              <SelectItem value="CNAME">CNAME — node hostname</SelectItem>
            </SelectContent>
          </Select>
        </Field>
      </div>
      <Field>
        <FieldLabel htmlFor={`${idPrefix}-record-value`}>Record value</FieldLabel>
        <Input
          id={`${idPrefix}-record-value`}
          placeholder="203.0.113.7 or node.example.com"
          value={form.recordValue}
          onChange={(event) => onChange({ ...form, recordValue: event.target.value })}
          required={requireSecrets}
        />
        <FieldDescription>
          Every provisioned domain becomes one record pointing here (the node serve address).
        </FieldDescription>
      </Field>
      <label className="flex items-center gap-2 text-sm">
        <Checkbox
          checked={form.proxied}
          onCheckedChange={(checked) => onChange({ ...form, proxied: checked === true })}
        />
        Proxied through Cloudflare (orange cloud)
      </label>
    </div>
  );
}

export function HostSourcesPanel() {
  const queryClient = useQueryClient();

  const sourcesQuery = useQuery({
    queryKey: ["admin", "host-sources"],
    queryFn: adminApi.listHostSources,
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "host-sources"] });

  const removeMutation = useMutation({
    mutationFn: (sourceId: string) => adminApi.removeHostSource(sourceId),
    onSuccess: invalidate,
  });
  const toggleDefaultMutation = useMutation({
    mutationFn: ({ sourceId, isDefault }: { sourceId: string; isDefault: boolean }) =>
      adminApi.updateHostSource(sourceId, { is_default: isDefault }),
    onSuccess: invalidate,
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-4">
        <p className="text-sm text-muted-foreground">
          Host sources provide platform domains. Wildcard sources activate immediately, DNS provider
          sources create records through the provider API (Cloudflare), and manual sources wait for
          operator DNS.
        </p>
        <CreateHostSourceDialog onCreated={invalidate} />
      </div>
      {sourcesQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
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
                  <TableCell className="font-medium">
                    {source.label}
                    {source.provider && (
                      <p className="text-xs text-muted-foreground capitalize">{source.provider}</p>
                    )}
                  </TableCell>
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
                    <EditHostSourceDialog source={source} onSaved={invalidate} />
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
  const [cloudflare, setCloudflare] = useState<CloudflareFormState>(emptyCloudflareForm);

  const createMutation = useMutation({
    mutationFn: () =>
      adminApi.createHostSource({
        label,
        kind,
        base_domain: baseDomain,
        is_default: isDefault,
        ...(kind === "dns_provider"
          ? {
              provider: "cloudflare",
              config: cloudflareConfig(cloudflare, { includeShape: true }),
            }
          : {}),
      }),
    onSuccess: () => {
      setOpen(false);
      setLabel("");
      setBaseDomain("");
      setIsDefault(false);
      setCloudflare(emptyCloudflareForm);
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
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Add host source</DialogTitle>
          <DialogDescription>
            Wildcard sources expect *.base-domain DNS pointing at the node serve listener. DNS
            provider sources create one record per domain via the provider API.
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
                <SelectItem value="dns_provider">DNS provider (Cloudflare)</SelectItem>
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
          {kind === "dns_provider" && (
            <CloudflareFields
              form={cloudflare}
              onChange={setCloudflare}
              requireSecrets
              idPrefix="source-cf"
            />
          )}
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={isDefault}
              onCheckedChange={(checked) => setIsDefault(checked === true)}
            />
            Use as the default source for automatic assignment
          </label>
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

function EditHostSourceDialog({
  source,
  onSaved,
}: {
  source: AdminHostSource;
  onSaved: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [label, setLabel] = useState(source.label);
  const [enabled, setEnabled] = useState(source.enabled);
  const [allowsAutoAssign, setAllowsAutoAssign] = useState(source.allows_auto_assign);
  const [cloudflare, setCloudflare] = useState<CloudflareFormState>(emptyCloudflareForm);
  const [shapeTouched, setShapeTouched] = useState(false);
  const isCloudflare = source.kind === "dns_provider";

  const updateMutation = useMutation({
    mutationFn: () => {
      const config = cloudflareConfig(cloudflare, { includeShape: shapeTouched });
      return adminApi.updateHostSource(source.id, {
        label,
        enabled,
        allows_auto_assign: allowsAutoAssign,
        ...(isCloudflare && Object.keys(config).length > 0
          ? { provider: "cloudflare", config }
          : {}),
      });
    },
    onSuccess: () => {
      setOpen(false);
      setCloudflare(emptyCloudflareForm);
      setShapeTouched(false);
      onSaved();
    },
  });

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) {
          setLabel(source.label);
          setEnabled(source.enabled);
          setAllowsAutoAssign(source.allows_auto_assign);
          setCloudflare(emptyCloudflareForm);
          setShapeTouched(false);
        }
      }}
    >
      <DialogTrigger asChild>
        <Button size="sm" variant="ghost" aria-label={`Edit ${source.label}`}>
          <PencilIcon />
        </Button>
      </DialogTrigger>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Edit host source</DialogTitle>
          <DialogDescription>
            {source.base_domain} · {source.kind.replace("_", " ")}
            {source.config_keys.length > 0 &&
              ` · configured keys: ${source.config_keys.join(", ")}`}
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (label.trim()) updateMutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor={`edit-label-${source.id}`}>Label</FieldLabel>
            <Input
              id={`edit-label-${source.id}`}
              value={label}
              onChange={(event) => setLabel(event.target.value)}
              required
            />
          </Field>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={enabled}
              onCheckedChange={(checked) => setEnabled(checked === true)}
            />
            Enabled
          </label>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={allowsAutoAssign}
              onCheckedChange={(checked) => setAllowsAutoAssign(checked === true)}
            />
            Allow automatic assignment to new projects
          </label>
          {isCloudflare && (
            <CloudflareFields
              form={cloudflare}
              onChange={(next) => {
                if (
                  next.recordType !== cloudflare.recordType ||
                  next.proxied !== cloudflare.proxied
                ) {
                  setShapeTouched(true);
                }
                setCloudflare(next);
              }}
              requireSecrets={false}
              idPrefix={`edit-cf-${source.id}`}
            />
          )}
          <DialogFooter>
            <Button type="submit" disabled={updateMutation.isPending}>
              {updateMutation.isPending ? "Saving…" : "Save changes"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
