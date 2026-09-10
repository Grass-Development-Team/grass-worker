import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlusIcon } from "lucide-react";
import { useId, useState } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { regionsApi, type Region } from "./regions.api";

export function NewRegionDialog({ onCreated }: { onCreated?: (region: Region) => void }) {
  const [open, setOpen] = useState(false);
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const id = useId();
  const client = useQueryClient();
  const create = useMutation({
    mutationFn: () => regionsApi.create({ code: code.trim(), name: name.trim() || undefined }),
    onSuccess: async ({ region }) => {
      await client.invalidateQueries({ queryKey: ["regions"] });
      onCreated?.(region);
      setOpen(false);
    },
  });
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        setOpen(value);
        if (value) {
          setCode("");
          setName("");
          create.reset();
        }
      }}
    >
      <DialogTrigger asChild>
        <Button type="button" variant="outline" size="sm">
          <PlusIcon data-icon="inline-start" />
          New Region
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New Region</DialogTitle>
          <DialogDescription>
            Create a region shared by nodes and regional entries.
          </DialogDescription>
        </DialogHeader>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            event.stopPropagation();
            create.mutate();
          }}
          className="flex flex-col gap-4"
        >
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor={id + "-code"}>Region code</FieldLabel>
              <Input
                id={id + "-code"}
                value={code}
                onChange={(e) => setCode(e.target.value)}
                placeholder="hk_1"
                required
                maxLength={64}
                pattern="[A-Za-z0-9_-]+"
              />
              <FieldDescription>
                Letters, numbers, underscores and hyphens. This code stays fixed.
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor={id + "-name"}>Display name (optional)</FieldLabel>
              <Input
                id={id + "-name"}
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Hong Kong"
                maxLength={128}
              />
            </Field>
          </FieldGroup>
          {create.isError && (
            <Alert variant="destructive">
              <AlertDescription>{create.error.message}</AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button type="submit" disabled={create.isPending || !code.trim()}>
              {create.isPending ? "Creating…" : "Create region"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function RegionSelect({
  id,
  value,
  onChange,
  allowCreate = false,
  unusedOnly = false,
  requireIngress = false,
  disabled = false,
}: {
  id: string;
  value: string;
  onChange: (code: string) => void;
  allowCreate?: boolean;
  unusedOnly?: boolean;
  requireIngress?: boolean;
  disabled?: boolean;
}) {
  const query = useQuery({ queryKey: ["regions"], queryFn: regionsApi.list });
  const regions = query.data?.regions ?? [];
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Select
          value={value}
          onValueChange={onChange}
          disabled={disabled || query.isPending || query.isError}
        >
          <SelectTrigger id={id} className="w-full">
            <SelectValue placeholder={query.isPending ? "Loading regions…" : "Select region"} />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {regions.map((region) => {
                const occupied = unusedOnly && Boolean(region.ingress_hostname);
                const unavailable =
                  requireIngress && (!region.ingress_hostname || !region.ingress_enabled);
                return (
                  <SelectItem
                    key={region.code}
                    value={region.code}
                    disabled={occupied || unavailable}
                  >
                    {region.name === region.code ? region.code : `${region.name} (${region.code})`}
                    {occupied
                      ? " — Entry already configured"
                      : unavailable
                        ? " — No enabled entry"
                        : ""}
                  </SelectItem>
                );
              })}
            </SelectGroup>
          </SelectContent>
        </Select>
        {allowCreate && !disabled && (
          <NewRegionDialog onCreated={(region) => onChange(region.code)} />
        )}
      </div>
      {query.isError && (
        <Alert variant="destructive">
          <AlertDescription>
            Regions could not be loaded.{" "}
            <Button type="button" variant="link" onClick={() => void query.refetch()}>
              Retry
            </Button>
          </AlertDescription>
        </Alert>
      )}
      {query.isSuccess && regions.length === 0 && (
        <FieldDescription>No regions configured yet.</FieldDescription>
      )}
      {requireIngress &&
        query.isSuccess &&
        !regions.some((r) => r.ingress_hostname && r.ingress_enabled) && (
          <FieldDescription>
            The platform has not configured any enabled regional entries. Contact an administrator.
          </FieldDescription>
        )}
    </div>
  );
}
