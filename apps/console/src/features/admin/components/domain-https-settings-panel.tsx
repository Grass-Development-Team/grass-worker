import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { SettingsCard } from "@/components/settings-card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
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
import { request } from "@/lib/api";

type Settings = { issuer: "letsencrypt" | "zerossl"; zerossl_eab_configured: boolean };
export function DomainHttpsSettingsPanel() {
  const query = useQuery({
    queryKey: ["admin", "domain-https"],
    queryFn: () => request<Settings>("/api/v1/admin/domain-https"),
  });
  if (query.isPending) return <Skeleton className="h-40 w-full" />;
  if (query.isError)
    return (
      <Alert variant="destructive">
        <AlertDescription>Domain HTTPS settings could not be loaded.</AlertDescription>
      </Alert>
    );
  return <SettingsForm initial={query.data} />;
}
function SettingsForm({ initial }: { initial: Settings }) {
  const [issuer, setIssuer] = useState(initial.issuer);
  const [kid, setKid] = useState("");
  const [key, setKey] = useState("");
  const client = useQueryClient();
  const mutation = useMutation({
    mutationFn: () =>
      request<Settings>("/api/v1/admin/domain-https", {
        method: "PATCH",
        body: JSON.stringify({
          issuer,
          ...(kid.trim() ? { eab_kid: kid.trim() } : {}),
          ...(key.trim() ? { eab_hmac_key: key.trim() } : {}),
        }),
      }),
    onSuccess: () => {
      setKid("");
      setKey("");
      void client.invalidateQueries({ queryKey: ["admin", "domain-https"] });
    },
  });
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        mutation.mutate();
      }}
    >
      <SettingsCard
        title="Custom domain HTTPS"
        description="Automatically issue and renew certificates after a customer's domain is connected."
        action={
          <Button size="sm" type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving…" : "Save HTTPS settings"}
          </Button>
        }
      >
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="domain-https-issuer">Certificate authority</FieldLabel>
            <Select
              value={issuer}
              onValueChange={(value) => setIssuer(value as Settings["issuer"])}
            >
              <SelectTrigger id="domain-https-issuer">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="letsencrypt">Let's Encrypt</SelectItem>
                  <SelectItem value="zerossl">ZeroSSL</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
            <FieldDescription>
              New certificates and the next renewal use this authority. The contact email comes from
              the user who adds the domain.
            </FieldDescription>
          </Field>
          {issuer === "zerossl" && (
            <>
              <Field>
                <FieldLabel htmlFor="domain-https-kid">ZeroSSL EAB key ID</FieldLabel>
                <Input
                  id="domain-https-kid"
                  type="password"
                  autoComplete="new-password"
                  value={kid}
                  onChange={(e) => setKid(e.target.value)}
                  required={!initial.zerossl_eab_configured}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="domain-https-key">ZeroSSL EAB HMAC key</FieldLabel>
                <Input
                  id="domain-https-key"
                  type="password"
                  autoComplete="new-password"
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
                  required={!initial.zerossl_eab_configured}
                />
                <FieldDescription>
                  {initial.zerossl_eab_configured
                    ? "Credentials are saved. Leave both fields empty to keep them."
                    : "ZeroSSL requires external account credentials."}
                </FieldDescription>
              </Field>
            </>
          )}
          <FieldDescription>
            Automatic renewal is enabled by default. HTTP validation requires the domain's public
            port 80 to reach an entry node.
          </FieldDescription>
        </FieldGroup>
        {mutation.isError && (
          <Alert variant="destructive">
            <AlertDescription>{mutation.error.message}</AlertDescription>
          </Alert>
        )}
        {mutation.isSuccess && (
          <p role="status" className="text-sm text-muted-foreground">
            HTTPS settings saved.
          </p>
        )}
      </SettingsCard>
    </form>
  );
}
