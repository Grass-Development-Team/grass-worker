import { useMutation } from "@tanstack/react-query";
import { useId } from "react";
import { CertificateImportDialog } from "@/components/certificate-import-dialog";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
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
  const renew = useMutation({
    mutationFn: () => projectsApi.renewHostCertificate(projectId, host.id),
    onSuccess: onChange,
  });
  const configure = useMutation({
    mutationFn: (input: {
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
          certificate?.https_ready
            ? "success"
            : certificate?.status === "failed"
              ? "destructive"
              : "secondary"
        }
      >
        {certificate?.https_ready
          ? "HTTPS ready"
          : certificate?.status === "active"
            ? "Installing certificate"
            : (certificate?.status ?? "Waiting for connection")}
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
      {(renew.isError || configure.isError) && (
        <Alert variant="destructive">
          <AlertDescription>{renew.error?.message ?? configure.error?.message}</AlertDescription>
        </Alert>
      )}
      {certificate && (
        <details>
          <summary className="cursor-pointer text-xs">Certificate settings</summary>
          <FieldGroup className="mt-2">
            <p className="text-xs text-muted-foreground">
              Certificate authority: {certificate.issuer}
            </p>
            <FieldDescription>
              Issued automatically after DNS and ownership verification. Keep public port 80
              reachable for validation and renewal.
            </FieldDescription>
            {canEdit && certificate.issuer !== "manual" && (
              <Field orientation="horizontal">
                <Checkbox
                  id={id + "-renew"}
                  checked={certificate.auto_renew}
                  disabled={configure.isPending || certificate.status === "issuing"}
                  onCheckedChange={(value) =>
                    configure.mutate({ certificate_auto_renew: value === true })
                  }
                />
                <FieldLabel htmlFor={id + "-renew"}>Automatic renewal</FieldLabel>
              </Field>
            )}
            {canEdit && host.ownership_status === "verified" && (
              <div className="flex flex-wrap gap-2">
                {certificate.issuer === "manual" ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={configure.isPending}
                    onClick={() =>
                      configure.mutate({
                        certificate_issuer: certificate.platform_issuer,
                        certificate_auto_renew: true,
                      })
                    }
                  >
                    Use managed certificate
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={renew.isPending || certificate.status === "issuing"}
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
          </FieldGroup>
        </details>
      )}
    </div>
  );
}
