import { useMutation } from "@tanstack/react-query";
import { useId } from "react";

import { CertificateImportDialog } from "@/components/certificate-import-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import { projectsApi, type DomainCertificate, type ProjectHost } from "./projects.api";

export function DomainCertificateControls({
  host,
  projectId,
  canEdit,
  onChange,
}: {
  host: ProjectHost;
  projectId: string;
  canEdit: boolean;
  onChange: () => void;
}) {
  const id = useId();
  const certificate = host.certificate;
  const verified = host.ownership_status === "verified";
  const renew = useMutation({
    mutationFn: () => projectsApi.renewHostCertificate(projectId, host.id),
    onSuccess: onChange,
  });
  const configure = useMutation({
    mutationFn: (input: {
      challenge_method?: "http01" | "dns01";
      certificate_auto_renew?: boolean;
      certificate_issuer?: DomainCertificate["issuer"];
    }) => projectsApi.updateHostCertificate(projectId, host.id, input),
    onSuccess: onChange,
  });
  if (host.kind !== "custom") return <span className="text-muted-foreground">Platform domain</span>;
  return (
    <div className="flex max-w-sm flex-col gap-2">
      <Badge
        variant={
          certificate?.status === "active"
            ? "success"
            : certificate?.status === "failed" || certificate?.status === "expiring"
              ? "destructive"
              : "secondary"
        }
      >
        {certificate?.status ?? "Awaiting verification"}
      </Badge>
      {certificate?.expires_at && (
        <p className="text-xs">Expires {new Date(certificate.expires_at).toLocaleString()}</p>
      )}
      {certificate?.error && <p className="text-xs text-destructive">{certificate.error}</p>}
      {certificate?.retry_at && (
        <p className="text-xs text-muted-foreground">
          Next attempt {new Date(certificate.retry_at).toLocaleString()}
        </p>
      )}
      {certificate && (
        <details>
          <summary className="cursor-pointer text-xs">Certificate settings</summary>
          <FieldGroup className="mt-2">
            <Field>
              <FieldLabel htmlFor={id + "-method"}>Certificate validation</FieldLabel>
              {canEdit ? (
                <Select
                  value={certificate.challenge_method}
                  disabled={configure.isPending}
                  onValueChange={(value) =>
                    configure.mutate({ challenge_method: value as "http01" | "dns01" })
                  }
                >
                  <SelectTrigger id={id + "-method"}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="http01">HTTP validation</SelectItem>
                      <SelectItem value="dns01">DNS delegation</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              ) : (
                <p>
                  {certificate.challenge_method === "dns01" ? "DNS delegation" : "HTTP validation"}
                </p>
              )}
              <FieldDescription>
                {certificate.challenge_method === "http01"
                  ? "The domain must point at the regional entry and accept HTTP traffic on port 80."
                  : "Publish the validation CNAME below. The platform manages challenge TXT records in its own zone."}
              </FieldDescription>
            </Field>
            {certificate.challenge_method === "dns01" &&
              certificate.dns_delegation_name &&
              certificate.dns_delegation_target && (
                <p className="break-all font-mono text-xs">
                  CNAME {certificate.dns_delegation_name} → {certificate.dns_delegation_target}
                </p>
              )}
            <p className="text-xs text-muted-foreground">
              Certificate authority: {certificate.issuer}
            </p>
            {canEdit && certificate.issuer !== "manual" && (
              <Field orientation="horizontal">
                <Checkbox
                  id={id + "-renew"}
                  checked={certificate.auto_renew}
                  disabled={configure.isPending}
                  onCheckedChange={(value) =>
                    configure.mutate({ certificate_auto_renew: value === true })
                  }
                />
                <FieldLabel htmlFor={id + "-renew"}>Automatic renewal</FieldLabel>
              </Field>
            )}
          </FieldGroup>
        </details>
      )}
      {canEdit && verified && (
        <div className="flex flex-wrap gap-2">
          {certificate?.issuer === "manual" &&
            certificate.regional_issuer &&
            certificate.regional_issuer !== "manual" && (
              <Button
                size="sm"
                variant="outline"
                disabled={configure.isPending}
                onClick={() =>
                  configure.mutate({
                    certificate_issuer: certificate.regional_issuer,
                    certificate_auto_renew: true,
                  })
                }
              >
                Use managed certificate
              </Button>
            )}
          {certificate?.issuer !== "manual" && (
            <Button
              size="sm"
              variant="outline"
              disabled={renew.isPending}
              onClick={() => renew.mutate()}
            >
              Renew certificate
            </Button>
          )}
          <CertificateImportDialog
            hostname={host.host}
            onImport={(input) => projectsApi.importHostCertificate(projectId, host.id, input)}
            onImported={onChange}
          />
        </div>
      )}
    </div>
  );
}
