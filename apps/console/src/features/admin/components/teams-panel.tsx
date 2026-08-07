import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDownIcon, MoreHorizontalIcon, PlusIcon, Trash2Icon } from "lucide-react";
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
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Field, FieldLabel } from "@/components/ui/field";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { adminApi, type AdminTeam } from "../admin.api";
import { showBatchResultToast } from "../batch-result-toast";

type TeamAction =
  | { kind: "detail"; team: AdminTeam }
  | { kind: "rename"; team: AdminTeam }
  | { kind: "group"; team: AdminTeam }
  | { kind: "plan"; team: AdminTeam }
  | { kind: "delete"; team: AdminTeam };

export function TeamsPanel() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<AdminTeam["kind"] | undefined>();
  const [groupId, setGroupId] = useState<string | undefined>();
  const [quotaPlanId, setQuotaPlanId] = useState<string | undefined>();
  const [action, setAction] = useState<TeamAction | null>(null);
  const [creating, setCreating] = useState(false);
  const [selectedTeamIds, setSelectedTeamIds] = useState<Set<string>>(new Set());

  const filters = {
    ...(query ? { q: query } : {}),
    ...(kind ? { kind } : {}),
    ...(groupId ? { group_id: groupId } : {}),
    ...(quotaPlanId ? { quota_plan_id: quotaPlanId } : {}),
  };

  const teamsQuery = useQuery({
    queryKey: ["admin", "teams", filters],
    queryFn: () => adminApi.listTeams(filters),
  });
  const groupsQuery = useQuery({
    queryKey: ["admin", "team-groups"],
    queryFn: adminApi.listTeamGroups,
  });
  const plansQuery = useQuery({
    queryKey: ["admin", "quota-plans"],
    queryFn: adminApi.listQuotaPlans,
  });
  const visibleTeamIds = teamsQuery.data?.teams.map((team) => team.id) ?? [];
  const selectedVisibleCount = visibleTeamIds.filter((id) => selectedTeamIds.has(id)).length;
  const allVisibleSelected =
    visibleTeamIds.length > 0 && selectedVisibleCount === visibleTeamIds.length;

  const close = () => setAction(null);
  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "teams"] });

  const batchMutation = useMutation({
    mutationFn: (
      input:
        | { action: "delete" }
        | { action: "assign_group"; group_id: string }
        | { action: "assign_quota_plan"; plan_id: string | null },
    ) => adminApi.batchTeams({ ...input, ids: [...selectedTeamIds] }),
    onSuccess: ({ results }, input) => {
      showBatchResultToast(
        results,
        results.length === 1 ? "team" : "teams",
        input.action === "delete" ? "deleted" : "updated",
      );
      setSelectedTeamIds(new Set());
      invalidate();
    },
  });

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
        <p className="text-sm text-muted-foreground">
          All teams on this platform, including personal teams.
        </p>
        <div className="flex flex-wrap items-center justify-end gap-2">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              setQuery(search);
              setSelectedTeamIds(new Set());
            }}
          >
            <Input
              aria-label="Search teams"
              placeholder="Search slug or name…"
              className="w-64"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </form>
          <Select
            value={kind ?? "all"}
            onValueChange={(value) => {
              setKind(value === "all" ? undefined : (value as AdminTeam["kind"]));
              setSelectedTeamIds(new Set());
            }}
          >
            <SelectTrigger aria-label="Team kind" size="sm">
              <SelectValue placeholder="All kinds" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="all">All kinds</SelectItem>
                <SelectItem value="team">Team</SelectItem>
                <SelectItem value="personal">Personal</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
          <Select
            value={groupId ?? "all"}
            onValueChange={(value) => {
              setGroupId(value === "all" ? undefined : value);
              setSelectedTeamIds(new Set());
            }}
          >
            <SelectTrigger aria-label="Team group" size="sm">
              <SelectValue placeholder="All groups" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="all">All groups</SelectItem>
                {groupsQuery.data?.groups.map((group) => (
                  <SelectItem key={group.id} value={group.id}>
                    {group.name}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <Select
            value={quotaPlanId ?? "all"}
            onValueChange={(value) => {
              setQuotaPlanId(value === "all" ? undefined : value);
              setSelectedTeamIds(new Set());
            }}
          >
            <SelectTrigger aria-label="Quota plan" size="sm">
              <SelectValue placeholder="All quota plans" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="all">All quota plans</SelectItem>
                {plansQuery.data?.plans.map((plan) => (
                  <SelectItem key={plan.id} value={plan.id}>
                    {plan.name}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <Button onClick={() => setCreating(true)}>
            <PlusIcon /> New team
          </Button>
        </div>
      </div>

      {selectedTeamIds.size > 0 && (
        <div className="flex min-h-10 items-center justify-between gap-4 border-y px-1 py-2">
          <p className="text-sm font-medium">{selectedTeamIds.size} teams selected</p>
          <div className="flex items-center gap-2">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  aria-label="Bulk actions"
                  disabled={batchMutation.isPending}
                >
                  Bulk actions <ChevronDownIcon />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuGroup>
                  <DropdownMenuSub>
                    <DropdownMenuSubTrigger>Assign group</DropdownMenuSubTrigger>
                    <DropdownMenuSubContent>
                      {groupsQuery.data?.groups.map((group) => (
                        <DropdownMenuItem
                          key={group.id}
                          onClick={() =>
                            batchMutation.mutate({ action: "assign_group", group_id: group.id })
                          }
                        >
                          {group.name}
                        </DropdownMenuItem>
                      ))}
                    </DropdownMenuSubContent>
                  </DropdownMenuSub>
                  <DropdownMenuSub>
                    <DropdownMenuSubTrigger>Assign quota plan</DropdownMenuSubTrigger>
                    <DropdownMenuSubContent>
                      <DropdownMenuItem
                        onClick={() =>
                          batchMutation.mutate({ action: "assign_quota_plan", plan_id: null })
                        }
                      >
                        Inherit from group
                      </DropdownMenuItem>
                      {plansQuery.data?.plans
                        .filter((plan) => plan.enabled)
                        .map((plan) => (
                          <DropdownMenuItem
                            key={plan.id}
                            onClick={() =>
                              batchMutation.mutate({
                                action: "assign_quota_plan",
                                plan_id: plan.id,
                              })
                            }
                          >
                            {plan.name}
                          </DropdownMenuItem>
                        ))}
                    </DropdownMenuSubContent>
                  </DropdownMenuSub>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  variant="destructive"
                  onClick={() => batchMutation.mutate({ action: "delete" })}
                >
                  <Trash2Icon data-icon="inline-start" /> Delete selected
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => setSelectedTeamIds(new Set())}
            >
              Clear selection
            </Button>
          </div>
        </div>
      )}

      {teamsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {teamsQuery.data && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-10">
                <Checkbox
                  aria-label="Select all visible teams"
                  checked={
                    allVisibleSelected ? true : selectedVisibleCount > 0 ? "indeterminate" : false
                  }
                  onCheckedChange={(checked) =>
                    setSelectedTeamIds((current) => {
                      const next = new Set(current);
                      for (const id of visibleTeamIds) {
                        if (checked === true) next.add(id);
                        else next.delete(id);
                      }
                      return next;
                    })
                  }
                />
              </TableHead>
              <TableHead>Team</TableHead>
              <TableHead>Kind</TableHead>
              <TableHead>Group</TableHead>
              <TableHead>Members</TableHead>
              <TableHead>Created</TableHead>
              <TableHead className="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {teamsQuery.data.teams.map((team) => (
              <TableRow key={team.id}>
                <TableCell>
                  <Checkbox
                    aria-label={`Select ${team.name}`}
                    checked={selectedTeamIds.has(team.id)}
                    onCheckedChange={(checked) =>
                      setSelectedTeamIds((current) => {
                        const next = new Set(current);
                        if (checked === true) next.add(team.id);
                        else next.delete(team.id);
                        return next;
                      })
                    }
                  />
                </TableCell>
                <TableCell>
                  <span className="font-medium">{team.name}</span>
                  <p className="text-xs text-muted-foreground">{team.slug}</p>
                </TableCell>
                <TableCell>
                  {team.kind === "personal" ? (
                    <Badge variant="secondary">Personal</Badge>
                  ) : (
                    <Badge variant="outline">Team</Badge>
                  )}
                </TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {team.group?.name ?? "—"}
                </TableCell>
                <TableCell className="text-sm text-muted-foreground">{team.member_count}</TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {new Date(team.created_at).toLocaleDateString()}
                </TableCell>
                <TableCell className="text-right">
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button size="sm" variant="ghost" aria-label={`Actions for ${team.name}`}>
                        <MoreHorizontalIcon />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuLabel>{team.name}</DropdownMenuLabel>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem onClick={() => setAction({ kind: "detail", team })}>
                        View details
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={() => setAction({ kind: "rename", team })}>
                        Rename
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={() => setAction({ kind: "group", team })}>
                        Change group
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={() => setAction({ kind: "plan", team })}>
                        Override quota plan
                      </DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        disabled={team.kind === "personal"}
                        onClick={() => setAction({ kind: "delete", team })}
                        className="text-destructive"
                      >
                        Delete team
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      {creating && (
        <CreateTeamDialog
          onClose={() => setCreating(false)}
          onCreated={() => {
            setCreating(false);
            invalidate();
          }}
        />
      )}
      {action?.kind === "detail" && <TeamDetailDialog team={action.team} onClose={close} />}
      {action?.kind === "rename" && (
        <RenameTeamDialog
          team={action.team}
          onClose={close}
          onSaved={() => {
            invalidate();
            close();
          }}
        />
      )}
      {action?.kind === "group" && (
        <ChangeGroupDialog
          team={action.team}
          onClose={close}
          onSaved={() => {
            invalidate();
            close();
          }}
        />
      )}
      {action?.kind === "plan" && (
        <OverridePlanDialog
          team={action.team}
          onClose={close}
          onSaved={() => {
            invalidate();
            close();
          }}
        />
      )}
      {action?.kind === "delete" && (
        <DeleteTeamDialog
          team={action.team}
          onClose={close}
          onDeleted={() => {
            invalidate();
            close();
          }}
        />
      )}
    </div>
  );
}

function TeamDetailDialog({ team, onClose }: { team: AdminTeam; onClose: () => void }) {
  const detailQuery = useQuery({
    queryKey: ["admin", "teams", "detail", team.id],
    queryFn: () => adminApi.teamDetail(team.id),
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{team.name}</DialogTitle>
          <DialogDescription>
            {team.slug} · {team.kind === "personal" ? "Personal team" : "Team"}
          </DialogDescription>
        </DialogHeader>
        {detailQuery.isLoading && <Skeleton className="h-32 w-full" aria-busy="true" />}
        {detailQuery.data && (
          <div className="space-y-4 text-sm">
            <div className="grid grid-cols-2 gap-2">
              <div>
                <p className="text-muted-foreground">Quota plan</p>
                <p className="font-medium">
                  {detailQuery.data.quota_plan.name}
                  <span className="ml-1 text-xs text-muted-foreground">
                    via {detailQuery.data.quota_plan.source}
                  </span>
                </p>
              </div>
              <div>
                <p className="text-muted-foreground">Projects</p>
                <p className="font-medium">{detailQuery.data.project_count}</p>
              </div>
            </div>
            <div>
              <p className="mb-1 text-muted-foreground">
                Members ({detailQuery.data.members.length})
              </p>
              <ul className="max-h-48 space-y-1 overflow-y-auto">
                {detailQuery.data.members.map((member) => (
                  <li key={member.user_id} className="flex items-center justify-between gap-2">
                    <span className="truncate">{member.display_name ?? member.email}</span>
                    <Badge variant="outline">{member.role}</Badge>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

function RenameTeamDialog({
  team,
  onClose,
  onSaved,
}: {
  team: AdminTeam;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(team.name);
  const mutation = useMutation({
    mutationFn: () => adminApi.updateTeam(team.id, { name }),
    onSuccess: onSaved,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Rename team</DialogTitle>
          <DialogDescription>{team.slug}</DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim()) mutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="admin-team-name">Team name</FieldLabel>
            <Input
              id="admin-team-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              required
            />
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ChangeGroupDialog({
  team,
  onClose,
  onSaved,
}: {
  team: AdminTeam;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [groupId, setGroupId] = useState(team.group?.id ?? "");
  const groupsQuery = useQuery({
    queryKey: ["admin", "team-groups"],
    queryFn: adminApi.listTeamGroups,
  });
  const mutation = useMutation({
    mutationFn: () => adminApi.assignTeamGroup(team.id, groupId),
    onSuccess: onSaved,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Change team group</DialogTitle>
          <DialogDescription>
            The group determines the quota plan and host policy for {team.name}.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (groupId) mutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="admin-team-group">Team group</FieldLabel>
            <Select value={groupId} onValueChange={setGroupId}>
              <SelectTrigger id="admin-team-group">
                <SelectValue placeholder="Select a group" />
              </SelectTrigger>
              <SelectContent>
                {groupsQuery.data?.groups.map((group) => (
                  <SelectItem key={group.id} value={group.id}>
                    {group.name} ({group.code})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending || !groupId}>
              {mutation.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function DeleteTeamDialog({
  team,
  onClose,
  onDeleted,
}: {
  team: AdminTeam;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const mutation = useMutation({
    mutationFn: () => adminApi.deleteTeam(team.id),
    onSuccess: onDeleted,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Delete team</DialogTitle>
          <DialogDescription>
            Soft-deletes “{team.name}”. Teams that still own projects are refused.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending}
          >
            {mutation.isPending ? "Deleting…" : "Delete team"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const INHERIT_GROUP = "__inherit__";

function OverridePlanDialog({
  team,
  onClose,
  onSaved,
}: {
  team: AdminTeam;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [planId, setPlanId] = useState(team.explicit_quota_plan_id ?? INHERIT_GROUP);
  const plansQuery = useQuery({
    queryKey: ["admin", "quota-plans"],
    queryFn: adminApi.listQuotaPlans,
  });
  const mutation = useMutation({
    mutationFn: () => adminApi.setTeamQuotaPlan(team.id, planId === INHERIT_GROUP ? null : planId),
    onSuccess: onSaved,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Override quota plan</DialogTitle>
          <DialogDescription>
            An explicit plan wins over the group plan for {team.name}.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            mutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="admin-team-plan">Quota plan</FieldLabel>
            <Select value={planId} onValueChange={setPlanId}>
              <SelectTrigger id="admin-team-plan">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={INHERIT_GROUP}>Inherit from team group</SelectItem>
                {plansQuery.data?.plans
                  .filter((plan) => plan.enabled)
                  .map((plan) => (
                    <SelectItem key={plan.id} value={plan.id}>
                      {plan.name} ({plan.code})
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function CreateTeamDialog({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [slugTouched, setSlugTouched] = useState(false);
  const [ownerId, setOwnerId] = useState("");

  const usersQuery = useQuery({
    queryKey: ["admin", "users", ""],
    queryFn: () => adminApi.listUsers(),
  });

  const mutation = useMutation({
    mutationFn: () =>
      adminApi.createTeam({ name, slug: slug || undefined, owner_user_id: ownerId }),
    onSuccess: onCreated,
  });

  const autoSlug = (value: string) =>
    value
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 60);

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New team</DialogTitle>
          <DialogDescription>
            The owner joins as team owner; the team lands in the default group.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim() && ownerId) mutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="new-team-name">Team name</FieldLabel>
            <Input
              id="new-team-name"
              value={name}
              onChange={(event) => {
                setName(event.target.value);
                if (!slugTouched) setSlug(autoSlug(event.target.value));
              }}
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="new-team-slug">Slug</FieldLabel>
            <Input
              id="new-team-slug"
              value={slug}
              onChange={(event) => {
                setSlugTouched(true);
                setSlug(event.target.value);
              }}
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="new-team-owner">Owner</FieldLabel>
            <Select value={ownerId} onValueChange={setOwnerId}>
              <SelectTrigger id="new-team-owner">
                <SelectValue placeholder="Select a user" />
              </SelectTrigger>
              <SelectContent>
                {usersQuery.data?.users
                  .filter((user) => user.status === "active")
                  .map((user) => (
                    <SelectItem key={user.id} value={user.id}>
                      {user.display_name ? `${user.display_name} (${user.email})` : user.email}
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending || !ownerId}>
              {mutation.isPending ? "Creating…" : "Create team"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
