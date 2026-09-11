import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { SettingsCard } from "@/components/settings-card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { NewRegionDialog } from "./region-select";
import { regionsApi, type Region } from "./regions.api";

function RegionRow({ region }: { region: Region }) {
  const [name, setName] = useState(region.name);
  const client = useQueryClient();
  const mutation = useMutation({
    mutationFn: (remove: boolean) =>
      remove ? regionsApi.remove(region.code) : regionsApi.rename(region.code, name),
    onSuccess: () => client.invalidateQueries({ queryKey: ["regions"] }),
  });
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        mutation.mutate(false);
      }}
    >
      <FieldGroup>
        <Field orientation="horizontal">
          <FieldLabel htmlFor={`region-name-${region.code}`}>{region.code}</FieldLabel>
          <Input
            id={`region-name-${region.code}`}
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            maxLength={128}
          />
          <Button size="sm" type="submit" disabled={mutation.isPending || name === region.name}>
            Save
          </Button>
          <Button
            size="sm"
            type="button"
            variant="outline"
            disabled={mutation.isPending || region.code === "default"}
            onClick={() => mutation.mutate(true)}
          >
            Remove
          </Button>
        </Field>
      </FieldGroup>
      {mutation.isError && (
        <Alert variant="destructive">
          <AlertDescription>{mutation.error.message}</AlertDescription>
        </Alert>
      )}
    </form>
  );
}
export function RegionsPanel() {
  const query = useQuery({ queryKey: ["regions"], queryFn: regionsApi.list });
  return (
    <SettingsCard
      title="Regions"
      description="Create reusable region tags for nodes and regional entries."
      action={<NewRegionDialog />}
    >
      <div className="flex flex-col gap-4">
        {query.isPending && <p>Loading regions…</p>}
        {query.isError && (
          <Alert variant="destructive">
            <AlertDescription>Regions could not be loaded.</AlertDescription>
          </Alert>
        )}
        {query.data?.regions?.map((region) => (
          <RegionRow key={region.code} region={region} />
        ))}
      </div>
    </SettingsCard>
  );
}
