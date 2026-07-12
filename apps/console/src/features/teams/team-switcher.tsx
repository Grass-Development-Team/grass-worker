import { useState } from "react";
import { CheckIcon, ChevronsUpDownIcon, PlusIcon } from "lucide-react";

import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

import { useTeam } from "./team-context";

function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("");
}

export function TeamSwitcher() {
  const { teams, activeTeam, isLoading, selectTeam, createTeam } = useTeam();
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);

  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!name.trim() || !slug.trim()) {
      setError("Team name and slug are required.");
      return;
    }

    setIsCreating(true);
    setError(null);
    try {
      await createTeam({ name: name.trim(), slug: slug.trim() });
      setName("");
      setSlug("");
      setCreateOpen(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to create team.");
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            className="h-auto w-full justify-start gap-2 px-2 py-2 text-left"
            disabled={isLoading}
          >
            <Avatar className="size-8 rounded-md">
              <AvatarFallback className="rounded-md text-xs">
                {activeTeam ? initials(activeTeam.name) : "GW"}
              </AvatarFallback>
            </Avatar>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-medium">
                {activeTeam?.name ?? (isLoading ? "Loading teams" : "No team")}
              </span>
              <span className="block truncate text-xs text-muted-foreground">
                {activeTeam?.kind === "personal" ? "Personal workspace" : "Team workspace"}
              </span>
            </span>
            <ChevronsUpDownIcon data-icon="inline-end" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent className="w-64" align="start">
          <DropdownMenuLabel>Teams</DropdownMenuLabel>
          <DropdownMenuGroup>
            {teams.map((team) => (
              <DropdownMenuItem key={team.id} onSelect={() => selectTeam(team.id)}>
                <Avatar className="size-6 rounded-md">
                  <AvatarFallback className="rounded-md text-[10px]">
                    {initials(team.name)}
                  </AvatarFallback>
                </Avatar>
                <span className="min-w-0 flex-1 truncate">{team.name}</span>
                {team.id === activeTeam?.id && <CheckIcon />}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
          <DropdownMenuSeparator />
          <DropdownMenuGroup>
            <DropdownMenuItem onSelect={() => setTimeout(() => setCreateOpen(true), 0)}>
              <PlusIcon />
              Create team
            </DropdownMenuItem>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <form className="flex flex-col gap-6" onSubmit={submit}>
            <DialogHeader>
              <DialogTitle>Create team</DialogTitle>
              <DialogDescription>
                Create a shared workspace for projects and deployments.
              </DialogDescription>
            </DialogHeader>
            <FieldGroup>
              <Field data-invalid={Boolean(error)}>
                <FieldLabel htmlFor="team-name">Team name</FieldLabel>
                <Input
                  id="team-name"
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  aria-invalid={Boolean(error)}
                  autoComplete="organization"
                />
              </Field>
              <Field data-invalid={Boolean(error)}>
                <FieldLabel htmlFor="team-slug">Team slug</FieldLabel>
                <Input
                  id="team-slug"
                  value={slug}
                  onChange={(event) => setSlug(event.target.value)}
                  aria-invalid={Boolean(error)}
                  autoCapitalize="none"
                  spellCheck={false}
                />
                {error && <FieldError>{error}</FieldError>}
              </Field>
            </FieldGroup>
            <DialogFooter className="gap-2">
              <Button type="button" variant="outline" onClick={() => setCreateOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={isCreating}>
                {isCreating && <Spinner data-icon="inline-start" />}
                Create team
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
