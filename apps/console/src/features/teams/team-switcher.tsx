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
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { showErrorToast } from "@/lib/toast";

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
  const [isCreating, setIsCreating] = useState(false);

  const resetCreateForm = () => {
    setName("");
    setSlug("");
  };

  const setCreateDialogOpen = (open: boolean) => {
    setCreateOpen(open);
    if (!open) resetCreateForm();
  };

  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!name.trim() || !slug.trim()) {
      showErrorToast(new Error("Team name and slug are required."));
      return;
    }

    setIsCreating(true);
    try {
      await createTeam({ name: name.trim(), slug: slug.trim() });
      setCreateDialogOpen(false);
    } catch (cause) {
      showErrorToast(cause);
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
            className="h-auto w-full justify-start gap-2 px-2 py-2 text-left group-data-[collapsible=icon]:size-8 group-data-[collapsible=icon]:p-0"
            disabled={isLoading}
            aria-label={activeTeam ? `Switch team, current team ${activeTeam.name}` : "Select team"}
          >
            <Avatar className="size-8 rounded-md">
              <AvatarFallback className="rounded-md text-xs">
                {activeTeam ? initials(activeTeam.name) : "GW"}
              </AvatarFallback>
            </Avatar>
            <span className="min-w-0 flex-1 group-data-[collapsible=icon]:hidden">
              <span className="block truncate text-sm font-medium">
                {activeTeam?.name ?? (isLoading ? "Loading teams" : "No team")}
              </span>
              <span className="block truncate text-xs text-muted-foreground">
                {activeTeam?.kind === "personal" ? "Personal workspace" : "Team workspace"}
              </span>
            </span>
            <ChevronsUpDownIcon
              data-icon="inline-end"
              className="group-data-[collapsible=icon]:hidden"
            />
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
            <DropdownMenuItem
              onSelect={() =>
                setTimeout(() => {
                  resetCreateForm();
                  setCreateOpen(true);
                }, 0)
              }
            >
              <PlusIcon />
              Create team
            </DropdownMenuItem>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>

      <Dialog open={createOpen} onOpenChange={setCreateDialogOpen}>
        <DialogContent>
          <form className="flex flex-col gap-6" onSubmit={submit}>
            <DialogHeader>
              <DialogTitle>Create team</DialogTitle>
              <DialogDescription>
                Create a shared workspace for projects and deployments.
              </DialogDescription>
            </DialogHeader>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="team-name">Team name</FieldLabel>
                <Input
                  id="team-name"
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  autoComplete="organization"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="team-slug">Team slug</FieldLabel>
                <Input
                  id="team-slug"
                  value={slug}
                  onChange={(event) => setSlug(event.target.value)}
                  autoCapitalize="none"
                  spellCheck={false}
                />
              </Field>
            </FieldGroup>
            <DialogFooter className="gap-2">
              <Button type="button" variant="outline" onClick={() => setCreateDialogOpen(false)}>
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
