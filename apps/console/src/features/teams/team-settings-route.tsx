import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { canEditTeam } from "./team-permissions";
import { teamKeys, useTeam } from "./team-context";
import { teamsApi } from "./teams.api";

export function TeamSettingsRoute() {
  const { activeTeam, activeRole, refreshTeams } = useTeam();
  const queryClient = useQueryClient();
  const detail = useQuery({
    queryKey: activeTeam ? teamKeys.detail(activeTeam.id) : ["teams", "none"],
    queryFn: () => teamsApi.get(activeTeam!.id),
    enabled: Boolean(activeTeam),
  });
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  useEffect(() => {
    if (detail.data) {
      setName(detail.data.team.name);
      setSlug(detail.data.team.slug);
    }
  }, [detail.data]);
  const mutation = useMutation({
    mutationFn: () => teamsApi.update(activeTeam!.id, { name: name.trim(), slug: slug.trim() }),
    onSuccess: async () => {
      await Promise.all([
        refreshTeams(),
        queryClient.invalidateQueries({ queryKey: teamKeys.detail(activeTeam!.id) }),
      ]);
    },
  });
  const editable = activeRole ? canEditTeam(activeRole) : false;

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold">Team settings</h1>
        <p className="text-sm text-muted-foreground">Manage the identity of this workspace.</p>
      </div>
      <Card>
        <CardHeader>
          <CardTitle>General</CardTitle>
          <CardDescription>Team name and URL slug.</CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="flex flex-col gap-6"
            onSubmit={(event) => {
              event.preventDefault();
              mutation.mutate();
            }}
          >
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="settings-name">Team name</FieldLabel>
                <Input
                  id="settings-name"
                  value={name}
                  disabled={!editable}
                  onChange={(event) => setName(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-slug">Team slug</FieldLabel>
                <Input
                  id="settings-slug"
                  value={slug}
                  disabled={!editable}
                  onChange={(event) => setSlug(event.target.value)}
                />
                <FieldDescription>Used as the stable workspace identifier.</FieldDescription>
              </Field>
              {mutation.error && <FieldError>{mutation.error.message}</FieldError>}
            </FieldGroup>
            {editable && (
              <Button
                className="self-start"
                disabled={!name.trim() || !slug.trim() || mutation.isPending}
              >
                {mutation.isPending && <Spinner data-icon="inline-start" />}Save changes
              </Button>
            )}
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
