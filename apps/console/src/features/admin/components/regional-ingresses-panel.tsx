import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Globe2Icon, PlusIcon, Trash2Icon } from "lucide-react";
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

import { adminApi, type AdminRegionalIngress } from "../admin.api";

function statusVariant(
  status: AdminRegionalIngress["certificate_status"],
): "success" | "warning" | "destructive" | "secondary" {
  if (status === "active") return "success";
  if (status === "failed" || status === "expiring") return "destructive";
  if (status === "pending" || status === "issuing") return "warning";
  return "secondary";
}

export function RegionalIngressesPanel() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: ["admin", "regional-ingresses"],
    queryFn: adminApi.listRegionalIngresses,
  });
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["admin", "regional-ingresses"] });
  const removeMutation = useMutation({
    mutationFn: adminApi.removeRegionalIngress,
    onSuccess: invalidate,
  });
  const toggleMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      adminApi.updateRegionalIngress(id, { enabled }),
    onSuccess: invalidate,
  });

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold">Regional ingresses</h2>
          <p className="text-sm text-muted-foreground">
            Each region exposes one CNAME target. Healthy Serve Nodes in that region remain
            available as entrance nodes and preserve the original Host header.
          </p>
        </div>
        <CreateRegionalIngressDialog onCreated={invalidate} />
      </div>
      {query.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {query.data &&
        (query.data.regional_ingresses.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <Globe2Icon className="mr-1 inline" />
            No regional ingress is configured.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Region</TableHead>
                <TableHead>Hostname</TableHead>
                <TableHead>TLS</TableHead>
                <TableHead>DNS-01</TableHead>
                <TableHead>Entrance nodes</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {query.data.regional_ingresses.map((item) => (
                <TableRow key={item.id}>
                  <TableCell className="font-mono">{item.region}</TableCell>
                  <TableCell>
                    <span className="font-mono text-sm">{item.hostname}</span>
                    {!item.enabled && <Badge variant="secondary">Disabled</Badge>}
                  </TableCell>
                  <TableCell>
                    <Badge variant={statusVariant(item.certificate_status)}>
                      {item.tls_enabled ? item.certificate_status : "off"}
                    </Badge>
                    <p className="text-xs text-muted-foreground">{item.certificate_issuer}</p>
                  </TableCell>
                  <TableCell>
                    <Badge
                      variant={item.dns_challenge_status === "valid" ? "success" : "secondary"}
                    >
                      {item.dns_challenge_status}
                    </Badge>
                    {item.dns_challenge_provider && (
                      <p className="text-xs text-muted-foreground">{item.dns_challenge_provider}</p>
                    )}
                  </TableCell>
                  <TableCell>{item.healthy_nodes.length}</TableCell>
                  <TableCell className="text-right">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => toggleMutation.mutate({ id: item.id, enabled: !item.enabled })}
                      disabled={toggleMutation.isPending}
                    >
                      {item.enabled ? "Disable" : "Enable"}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Remove ${item.region} ingress`}
                      onClick={() => removeMutation.mutate(item.id)}
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

function CreateRegionalIngressDialog({ onCreated }: { onCreated: () => void }) {
  const [open, setOpen] = useState(false);
  const [region, setRegion] = useState("default");
  const [hostname, setHostname] = useState("");
  const [healthPath, setHealthPath] = useState("/health");
  const [healthInterval, setHealthInterval] = useState("30");
  const [issuer, setIssuer] = useState<AdminRegionalIngress["certificate_issuer"]>("letsencrypt");
  const [challengeProvider, setChallengeProvider] = useState("");
  const [tlsEnabled, setTlsEnabled] = useState(true);
  const [autoRenew, setAutoRenew] = useState(true);
  const mutation = useMutation({
    mutationFn: () =>
      adminApi.createRegionalIngress({
        region,
        hostname,
        health_check_path: healthPath,
        health_check_interval_seconds: Number(healthInterval),
        tls_enabled: tlsEnabled,
        certificate_issuer: issuer,
        certificate_auto_renew: autoRenew,
        ...(challengeProvider.trim()
          ? { dns_challenge_provider: challengeProvider.trim(), dns_challenge_config: {} }
          : {}),
      }),
    onSuccess: () => {
      setOpen(false);
      setRegion("default");
      setHostname("");
      setHealthPath("/health");
      setHealthInterval("30");
      setIssuer("letsencrypt");
      setChallengeProvider("");
      setTlsEnabled(true);
      setAutoRenew(true);
      onCreated();
    },
  });

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>
          <PlusIcon /> Add ingress
        </Button>
      </DialogTrigger>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Add regional ingress</DialogTitle>
          <DialogDescription>
            The hostname is the CNAME target shown to project members. Certificate issuance and
            DNS-01 status remain visible while the configured ACME worker reconciles them.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (region.trim() && hostname.trim()) mutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="regional-ingress-region">Region</FieldLabel>
            <Input
              id="regional-ingress-region"
              value={region}
              onChange={(event) => setRegion(event.target.value)}
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="regional-ingress-hostname">Ingress hostname</FieldLabel>
            <Input
              id="regional-ingress-hostname"
              placeholder="eu-west.edge.example.com"
              value={hostname}
              onChange={(event) => setHostname(event.target.value)}
              required
            />
          </Field>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="regional-ingress-health-path">Health path</FieldLabel>
              <Input
                id="regional-ingress-health-path"
                value={healthPath}
                onChange={(event) => setHealthPath(event.target.value)}
                required
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="regional-ingress-health-interval">Interval seconds</FieldLabel>
              <Input
                id="regional-ingress-health-interval"
                type="number"
                min={5}
                max={3600}
                value={healthInterval}
                onChange={(event) => setHealthInterval(event.target.value)}
                required
              />
            </Field>
          </div>
          <Field>
            <FieldLabel htmlFor="regional-ingress-issuer">Certificate issuer</FieldLabel>
            <Select
              value={issuer}
              onValueChange={(value) =>
                setIssuer(value as AdminRegionalIngress["certificate_issuer"])
              }
            >
              <SelectTrigger id="regional-ingress-issuer">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="letsencrypt">Let's Encrypt</SelectItem>
                <SelectItem value="zerossl">ZeroSSL</SelectItem>
                <SelectItem value="manual">Manual</SelectItem>
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor="regional-ingress-challenge-provider">DNS-01 provider</FieldLabel>
            <Input
              id="regional-ingress-challenge-provider"
              placeholder="cloudflare, dnspod, route53"
              value={challengeProvider}
              onChange={(event) => setChallengeProvider(event.target.value)}
            />
          </Field>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={tlsEnabled}
              onCheckedChange={(checked) => setTlsEnabled(checked === true)}
            />
            Enable managed TLS
          </label>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={autoRenew}
              onCheckedChange={(checked) => setAutoRenew(checked === true)}
            />
            Renew certificates automatically
          </label>
          <DialogFooter>
            <Button
              type="submit"
              disabled={mutation.isPending || !region.trim() || !hostname.trim()}
            >
              {mutation.isPending ? "Creating…" : "Create ingress"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
