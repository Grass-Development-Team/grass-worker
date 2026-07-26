import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MoreHorizontalIcon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
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
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
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
  const [action, setAction] = useState<TeamAction | null>(null);

  const teamsQuery = useQuery({
    queryKey: ["admin", "teams", query],
    queryFn: () => adminApi.listTeams(query),
  });

  const close = () => setAction(null);
  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "teams"] });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-4">
        <p className="text-sm text-muted-foreground">
          All teams on this platform, including personal teams.
        </p>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            setQuery(search);
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
      </div>

      {teamsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {teamsQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {teamsQuery.error instanceof Error ? teamsQuery.error.message : "Unable to load teams."}
        </p>
      )}
      {teamsQuery.data && (
        <Table>
          <TableHeader>
            <TableRow>
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
        {detailQuery.isError && (
          <p role="alert" className="text-sm text-destructive">
            Unable to load team details.
          </p>
        )}
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
          {mutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : "Unable to rename."}
            </p>
          )}
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
          {mutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {mutation.error instanceof Error
                ? mutation.error.message
                : "Unable to change the group."}
            </p>
          )}
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
        {mutation.isError && (
          <p role="alert" className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : "Unable to delete."}
          </p>
        )}
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
          {mutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : "Unable to save."}
            </p>
          )}
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
