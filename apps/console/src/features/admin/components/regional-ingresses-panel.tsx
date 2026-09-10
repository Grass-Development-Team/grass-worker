import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Globe2Icon, PlusIcon, RefreshCwIcon, Trash2Icon } from "lucide-react";
import { useId, useState } from "react";

import { RegionSelect } from "@/features/regions/region-select";
import { CertificateImportDialog } from "@/components/certificate-import-dialog";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { adminApi, type AdminRegionalIngress } from "../admin.api";

function statusVariant(
  status: AdminRegionalIngress["certificate_status"],
): "success" | "warning" | "destructive" | "secondary" {
  if (status === "active") return "success";
  if (status === "failed" || status === "expiring") return "destructive";
  if (status === "pending" || status === "issuing") return "warning";
  return "secondary";
}

function displayTime(value?: string | null) {
  return value ? new Date(value).toLocaleString() : "—";
}

export function RegionalIngressesPanel() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: ["admin", "regional-ingresses"],
    queryFn: adminApi.listRegionalIngresses,
    refetchInterval: 10_000,
  });
  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["admin", "regional-ingresses"] });
  };
  const removeMutation = useMutation({
    mutationFn: adminApi.removeRegionalIngress,
    onSuccess: invalidate,
  });
  const toggleMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      adminApi.updateRegionalIngress(id, { enabled }),
    onSuccess: invalidate,
  });
  const renewMutation = useMutation({
    mutationFn: adminApi.renewRegionalIngressCertificate,
    onSuccess: invalidate,
  });

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold">Regional ingresses</h2>
          <p className="text-sm text-muted-foreground">
            Manage regional entry domains, certificates, and healthy entry nodes.
          </p>
        </div>
        <RegionalIngressDialog onSaved={invalidate} />
      </div>
      {query.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {query.isError && (
        <Alert variant="destructive">
          <AlertDescription>Regional ingresses could not be loaded.</AlertDescription>
        </Alert>
      )}
      {query.data &&
        (query.data.regional_ingresses.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Globe2Icon />
              </EmptyMedia>
              <EmptyTitle>No regional ingresses</EmptyTitle>
              <EmptyDescription>
                Add an entry domain to enable regional CNAME guidance and automated certificates.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Region / hostname</TableHead>
                <TableHead>Certificate</TableHead>
                <TableHead>DNS validation</TableHead>
                <TableHead>Entry nodes</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {query.data.regional_ingresses.map((item) => (
                <TableRow key={item.id}>
                  <TableCell>
                    <p className="font-mono">{item.region}</p>
                    <p>{item.hostname}</p>
                    {!item.enabled && <Badge variant="secondary">Disabled</Badge>}
                  </TableCell>
                  <TableCell>
                    <Badge variant={statusVariant(item.certificate_status)}>
                      {item.tls_enabled ? item.certificate_status : "off"}
                    </Badge>
                    <p className="text-xs text-muted-foreground">
                      {item.certificate_issuer} ·{" "}
                      {item.certificate_auto_renew ? "Auto renewal" : "Manual renewal"}
                    </p>
                    <p className="text-xs">Expires {displayTime(item.certificate_expires_at)}</p>
                    {item.certificate_retry_at && (
                      <p className="text-xs">
                        Next attempt {displayTime(item.certificate_retry_at)}
                      </p>
                    )}
                    {item.certificate_error && (
                      <p className="max-w-sm text-xs text-destructive">{item.certificate_error}</p>
                    )}
                  </TableCell>
                  <TableCell>
                    <Badge
                      variant={item.dns_challenge_status === "valid" ? "success" : "secondary"}
                    >
                      {item.dns_challenge_status}
                    </Badge>
                    <p className="text-xs text-muted-foreground">
                      {item.dns_challenge_provider ?? "Not configured"}
                    </p>
                  </TableCell>
                  <TableCell>
                    <p>{item.healthy_nodes.length} healthy</p>
                    <p className="text-xs text-muted-foreground">
                      {
                        (item.node_statuses ?? []).filter(
                          (node) =>
                            node.tls_ready &&
                            item.healthy_nodes.some(
                              (healthy) => healthy.node_id === node.node_id,
                            ) &&
                            !!item.certificate_revision &&
                            node.certificate_revision === item.certificate_revision,
                        ).length
                      }{" "}
                      using current certificate
                    </p>
                    {(item.node_statuses?.length ?? 0) > 0 && (
                      <details className="mt-1 text-xs">
                        <summary>Node status</summary>
                        <ul>
                          {item.node_statuses?.map((node) => (
                            <li key={node.node_id}>
                              {node.node_id} · {node.tls_ready ? "TLS ready" : "TLS unavailable"} ·{" "}
                              {displayTime(node.checked_at)}
                            </li>
                          ))}
                        </ul>
                      </details>
                    )}
                  </TableCell>
                  <TableCell>
                    <div className="flex flex-wrap justify-end gap-2">
                      <RegionalIngressDialog item={item} onSaved={invalidate} />
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={
                          renewMutation.isPending ||
                          !item.enabled ||
                          !item.tls_enabled ||
                          item.certificate_issuer === "manual"
                        }
                        onClick={() => renewMutation.mutate(item.id)}
                      >
                        <RefreshCwIcon data-icon="inline-start" />
                        Renew / retry
                      </Button>
                      <CertificateImportDialog
                        hostname={item.hostname}
                        onImport={(input) =>
                          adminApi.importRegionalIngressCertificate(item.id, input)
                        }
                        onImported={invalidate}
                      />
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() =>
                          toggleMutation.mutate({ id: item.id, enabled: !item.enabled })
                        }
                        disabled={toggleMutation.isPending}
                      >
                        {item.enabled ? "Disable" : "Enable"}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={"Remove " + item.region + " ingress"}
                        onClick={() => removeMutation.mutate(item.id)}
                        disabled={removeMutation.isPending}
                      >
                        <Trash2Icon data-icon="inline-start" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}
    </div>
  );
}

type Provider = "cloudflare" | "dnspod" | "route53";
const providerFields: Record<
  Provider,
  Array<{ key: string; label: string; secret?: boolean; optional?: boolean }>
> = {
  cloudflare: [
    { key: "api_token", label: "Cloudflare API token", secret: true },
    { key: "zone_id", label: "Cloudflare zone ID" },
    { key: "zone", label: "DNS zone", optional: true },
  ],
  dnspod: [
    { key: "secret_id", label: "Tencent Cloud secret ID", secret: true },
    { key: "secret_key", label: "Tencent Cloud secret key", secret: true },
    { key: "domain", label: "DNSPod domain" },
  ],
  route53: [
    { key: "access_key_id", label: "AWS access key ID", secret: true },
    { key: "secret_access_key", label: "AWS secret access key", secret: true },
    { key: "hosted_zone_id", label: "Route53 hosted zone ID" },
    { key: "zone", label: "DNS zone", optional: true },
    { key: "region", label: "AWS signing region", optional: true },
  ],
};

function RegionalIngressDialog({
  item,
  onSaved,
}: {
  item?: AdminRegionalIngress;
  onSaved: () => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size={item ? "sm" : "default"} variant={item ? "outline" : "default"}>
          {!item && <PlusIcon data-icon="inline-start" />}
          {item ? "Edit" : "Add regional ingress"}
        </Button>
      </DialogTrigger>
      <DialogContent className="max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{item ? "Edit regional ingress" : "Add regional ingress"}</DialogTitle>
          <DialogDescription>
            Configure the entry domain and certificate authority for this region.
          </DialogDescription>
        </DialogHeader>
        {open && (
          <IngressForm
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

function IngressForm({ item, onSaved }: { item?: AdminRegionalIngress; onSaved: () => void }) {
  const id = useId();
  const [region, setRegion] = useState(item?.region ?? "");
  const [hostname, setHostname] = useState(item?.hostname ?? "");
  const [healthPath, setHealthPath] = useState(item?.health_check_path ?? "/_grass/health");
  const [healthInterval, setHealthInterval] = useState(
    String(item?.health_check_interval_seconds ?? 30),
  );
  const [issuer, setIssuer] = useState<AdminRegionalIngress["certificate_issuer"]>(
    item?.certificate_issuer ?? "letsencrypt",
  );
  const [provider, setProvider] = useState<Provider>(
    (item?.dns_challenge_provider as Provider) in providerFields
      ? (item!.dns_challenge_provider as Provider)
      : "cloudflare",
  );
  const [tlsEnabled, setTlsEnabled] = useState(item?.tls_enabled ?? true);
  const [autoRenew, setAutoRenew] = useState(item?.certificate_auto_renew ?? true);
  const [config, setConfig] = useState<Record<string, string | null>>({});
  const configured =
    item?.dns_challenge_provider === provider ? item.dns_challenge_config_keys : [];
  const fields = [
    ...providerFields[provider],
    { key: "contact_email", label: "Certificate contact email", optional: true },
    ...(issuer === "zerossl"
      ? [
          { key: "eab_kid", label: "ZeroSSL EAB key ID", secret: true },
          { key: "eab_hmac_key", label: "ZeroSSL EAB HMAC key", secret: true },
        ]
      : []),
  ];
  const mutation = useMutation({
    mutationFn: () => {
      const dnsConfig = Object.fromEntries(
        Object.entries(config)
          .filter(([, value]) => value === null || value.trim())
          .map(([key, value]) => [key, value?.trim() ?? null]),
      );
      const input = {
        hostname: hostname.trim(),
        health_check_path: healthPath.trim(),
        health_check_interval_seconds: Number(healthInterval),
        tls_enabled: tlsEnabled,
        certificate_issuer: issuer,
        certificate_auto_renew: issuer !== "manual" && autoRenew,
        ...(tlsEnabled && issuer !== "manual"
          ? { dns_challenge_provider: provider, dns_challenge_config: dnsConfig }
          : {}),
      };
      return item
        ? adminApi.updateRegionalIngress(item.id, input)
        : adminApi.createRegionalIngress({ ...input, region: region.trim() });
    },
    onSuccess: onSaved,
  });
  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        mutation.mutate();
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
          <FieldLabel htmlFor={id + "-hostname"}>Entry hostname</FieldLabel>
          <Input
            id={id + "-hostname"}
            value={hostname}
            onChange={(event) => setHostname(event.target.value)}
            placeholder="eu.edge.example.com"
            required
          />
        </Field>
        <FieldSet>
          <FieldLegend>Health checks</FieldLegend>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor={id + "-path"}>Health check path</FieldLabel>
              <Input
                id={id + "-path"}
                value={healthPath}
                onChange={(event) => setHealthPath(event.target.value)}
                required
                pattern="/.*"
              />
            </Field>
            <Field>
              <FieldLabel htmlFor={id + "-interval"}>Interval (seconds)</FieldLabel>
              <Input
                id={id + "-interval"}
                type="number"
                min={5}
                max={3600}
                value={healthInterval}
                onChange={(event) => setHealthInterval(event.target.value)}
                required
              />
            </Field>
          </FieldGroup>
        </FieldSet>
        <FieldSet>
          <FieldLegend>HTTPS</FieldLegend>
          <FieldGroup>
            <Field orientation="horizontal">
              <Checkbox
                id={id + "-tls"}
                checked={tlsEnabled}
                onCheckedChange={(value) => setTlsEnabled(value === true)}
              />
              <FieldLabel htmlFor={id + "-tls"}>Enable HTTPS</FieldLabel>
            </Field>
            {tlsEnabled && (
              <>
                <Field>
                  <FieldLabel htmlFor={id + "-issuer"}>Certificate authority</FieldLabel>
                  <Select
                    value={issuer}
                    onValueChange={(value) => setIssuer(value as typeof issuer)}
                  >
                    <SelectTrigger id={id + "-issuer"}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        <SelectItem value="letsencrypt">Let's Encrypt</SelectItem>
                        <SelectItem value="zerossl">ZeroSSL</SelectItem>
                        <SelectItem value="manual">Manual certificate</SelectItem>
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </Field>
                {issuer !== "manual" && (
                  <>
                    <Field orientation="horizontal">
                      <Checkbox
                        id={id + "-renew"}
                        checked={autoRenew}
                        onCheckedChange={(value) => setAutoRenew(value === true)}
                      />
                      <FieldLabel htmlFor={id + "-renew"}>
                        Automatically renew certificates
                      </FieldLabel>
                    </Field>
                    <Field>
                      <FieldLabel htmlFor={id + "-provider"}>DNS provider</FieldLabel>
                      <Select
                        value={provider}
                        onValueChange={(value) => {
                          setProvider(value as Provider);
                          setConfig({});
                        }}
                      >
                        <SelectTrigger id={id + "-provider"}>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectGroup>
                            <SelectItem value="cloudflare">Cloudflare</SelectItem>
                            <SelectItem value="dnspod">DNSPod</SelectItem>
                            <SelectItem value="route53">Route53</SelectItem>
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                      <FieldDescription>
                        Allow the provider to create and remove validation TXT records in the entry
                        domain's zone.
                      </FieldDescription>
                    </Field>
                    {fields.map((field) => (
                      <Field key={field.key}>
                        <FieldLabel htmlFor={id + "-" + field.key}>{field.label}</FieldLabel>
                        <Input
                          id={id + "-" + field.key}
                          type={
                            field.secret
                              ? "password"
                              : field.key === "contact_email"
                                ? "email"
                                : "text"
                          }
                          value={config[field.key] ?? ""}
                          disabled={config[field.key] === null}
                          onChange={(event) =>
                            setConfig({ ...config, [field.key]: event.target.value })
                          }
                          required={!field.optional && !configured.includes(field.key)}
                          autoComplete={field.secret ? "new-password" : "off"}
                          placeholder={
                            configured.includes(field.key)
                              ? "Configured — leave blank to keep"
                              : undefined
                          }
                        />
                        {field.optional && configured.includes(field.key) && (
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() =>
                              setConfig({
                                ...config,
                                [field.key]: config[field.key] === null ? "" : null,
                              })
                            }
                          >
                            {config[field.key] === null
                              ? `Keep ${field.label}`
                              : `Remove ${field.label}`}
                          </Button>
                        )}
                      </Field>
                    ))}
                  </>
                )}
              </>
            )}
          </FieldGroup>
        </FieldSet>
      </FieldGroup>
      <DialogFooter>
        <Button type="submit" disabled={mutation.isPending}>
          {mutation.isPending ? "Saving…" : item ? "Save changes" : "Create ingress"}
        </Button>
      </DialogFooter>
    </form>
  );
}
