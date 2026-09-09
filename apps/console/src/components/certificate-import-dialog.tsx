import { useMutation } from "@tanstack/react-query";
import { useId, useState } from "react";

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
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Textarea } from "@/components/ui/textarea";

export function CertificateImportDialog({
  hostname,
  onImport,
  onImported,
}: {
  hostname: string;
  onImport: (input: { certificate_pem: string; private_key_pem: string }) => Promise<unknown>;
  onImported: () => void;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [certificate, setCertificate] = useState("");
  const [privateKey, setPrivateKey] = useState("");
  const clear = () => {
    setCertificate("");
    setPrivateKey("");
  };
  const mutation = useMutation({
    mutationFn: () => onImport({ certificate_pem: certificate, private_key_pem: privateKey }),
    onSuccess: () => {
      clear();
      setOpen(false);
      onImported();
    },
  });
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        setOpen(value);
        if (!value) clear();
      }}
    >
      <DialogTrigger asChild>
        <Button size="sm" variant="outline">
          Import certificate
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Import certificate for {hostname}</DialogTitle>
          <DialogDescription>
            The certificate must cover this hostname and match its private key. Imported
            certificates are renewed manually.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            mutation.mutate();
          }}
        >
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor={id + "-certificate"}>Certificate chain (PEM)</FieldLabel>
              <Textarea
                id={id + "-certificate"}
                value={certificate}
                onChange={(event) => setCertificate(event.target.value)}
                required
                rows={5}
                spellCheck={false}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor={id + "-key"}>Private key (PEM)</FieldLabel>
              <Textarea
                id={id + "-key"}
                value={privateKey}
                onChange={(event) => setPrivateKey(event.target.value)}
                required
                rows={4}
                spellCheck={false}
                autoComplete="off"
              />
              <FieldDescription>
                The key is encrypted when stored and is never returned by this form.
              </FieldDescription>
            </Field>
          </FieldGroup>
          <DialogFooter>
            <Button
              type="submit"
              disabled={mutation.isPending || !certificate.trim() || !privateKey.trim()}
            >
              {mutation.isPending ? "Importing…" : "Import"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
