import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CopyIcon, MoreHorizontalIcon, PlusIcon } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { teamKeys, useTeam } from "./team-context";
import { canManageMembers } from "./team-permissions";
import { teamsApi, type ManagedTeamRole, type TeamInvitation, type TeamMember } from "./teams.api";

const roles: ManagedTeamRole[] = ["admin", "member", "viewer"];

export function TeamMembersRoute() {
  const { activeTeam, activeRole } = useTeam();
  const queryClient = useQueryClient();
  const [inviteOpen, setInviteOpen] = useState(false);
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<ManagedTeamRole>("member");
  const [invitation, setInvitation] = useState<TeamInvitation | null>(null);
  const [removeTarget, setRemoveTarget] = useState<TeamMember | null>(null);
  const manageable = activeRole ? canManageMembers(activeRole) : false;
  const members = useQuery({
    queryKey: activeTeam ? teamKeys.members(activeTeam.id) : ["teams", "none", "members"],
    queryFn: () => teamsApi.listMembers(activeTeam!.id),
    enabled: Boolean(activeTeam),
  });
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: teamKeys.members(activeTeam!.id) });
  const invite = useMutation({
    mutationFn: () => teamsApi.inviteMember(activeTeam!.id, { email: email.trim(), role }),
    onSuccess: ({ invitation }) => setInvitation(invitation),
  });
  const updateRole = useMutation({
    mutationFn: ({ userId, role }: { userId: string; role: ManagedTeamRole }) =>
      teamsApi.updateMemberRole(activeTeam!.id, userId, role),
    onSuccess: refresh,
  });
  const remove = useMutation({
    mutationFn: (userId: string) => teamsApi.removeMember(activeTeam!.id, userId),
    onSuccess: async () => {
      setRemoveTarget(null);
      await refresh();
    },
  });
  const link = invitation
    ? `${window.location.origin}/invitations/accept?token=${encodeURIComponent(invitation.token)}`
    : "";

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <div className="flex items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">Members</h1>
          <p className="text-sm text-muted-foreground">Manage access to {activeTeam?.name}.</p>
        </div>
        {manageable && (
          <Button
            onClick={() => {
              setInvitation(null);
              setInviteOpen(true);
            }}
          >
            <PlusIcon data-icon="inline-start" />
            Invite member
          </Button>
        )}
      </div>
      <Card>
        <CardHeader>
          <CardTitle>Team members</CardTitle>
          <CardDescription>{members.data?.members.length ?? 0} people have access.</CardDescription>
        </CardHeader>
        <CardContent>
          {members.isLoading ? (
            <Spinner />
          ) : members.error ? (
            <p className="text-sm text-destructive">{members.error.message}</p>
          ) : members.data?.members.length === 0 ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              No members found for this team.
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>User</TableHead>
                  <TableHead>Role</TableHead>
                  <TableHead>Joined</TableHead>
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {members.data?.members.map((member) => (
                  <TableRow key={member.id}>
                    <TableCell>
                      <div className="font-medium">{member.display_name || member.email}</div>
                      <div className="text-xs text-muted-foreground">{member.email}</div>
                    </TableCell>
                    <TableCell>
                      {member.role === "owner" || !manageable ? (
                        <span className="capitalize">{member.role}</span>
                      ) : (
                        <Select
                          value={member.role}
                          onValueChange={(value) =>
                            updateRole.mutate({
                              userId: member.user_id,
                              role: value as ManagedTeamRole,
                            })
                          }
                        >
                          <SelectTrigger aria-label={`Role for ${member.email}`} className="w-32">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              {roles.map((role) => (
                                <SelectItem key={role} value={role}>
                                  {role}
                                </SelectItem>
                              ))}
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      )}
                    </TableCell>
                    <TableCell>{new Date(member.joined_at).toLocaleDateString()}</TableCell>
                    <TableCell>
                      {manageable && member.role !== "owner" && (
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              aria-label={`Actions for ${member.email}`}
                            >
                              <MoreHorizontalIcon />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end">
                            <DropdownMenuGroup>
                              <DropdownMenuItem
                                onSelect={() => setTimeout(() => setRemoveTarget(member), 0)}
                                className="text-destructive"
                              >
                                Remove member
                              </DropdownMenuItem>
                            </DropdownMenuGroup>
                          </DropdownMenuContent>
                        </DropdownMenu>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
      <Dialog open={inviteOpen} onOpenChange={setInviteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{invitation ? "Invitation link created" : "Invite member"}</DialogTitle>
            <DialogDescription>
              {invitation
                ? "This link is shown once. Copy it before closing."
                : "Invite someone to this team by email."}
            </DialogDescription>
          </DialogHeader>
          {invitation ? (
            <div className="flex gap-2">
              <Input value={link} readOnly aria-label="Invitation link" />
              <Button
                type="button"
                size="icon"
                onClick={() => navigator.clipboard.writeText(link)}
                aria-label="Copy invitation link"
              >
                <CopyIcon />
              </Button>
            </div>
          ) : (
            <form
              className="flex flex-col gap-6"
              onSubmit={(event) => {
                event.preventDefault();
                invite.mutate();
              }}
            >
              <FieldGroup>
                <Field>
                  <FieldLabel htmlFor="invite-email">Email</FieldLabel>
                  <Input
                    id="invite-email"
                    type="email"
                    required
                    value={email}
                    onChange={(event) => setEmail(event.target.value)}
                  />
                </Field>
                <Field>
                  <FieldLabel>Role</FieldLabel>
                  <Select value={role} onValueChange={(value) => setRole(value as ManagedTeamRole)}>
                    <SelectTrigger aria-label="Invitation role">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {roles.map((role) => (
                          <SelectItem key={role} value={role}>
                            {role}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </Field>
                {invite.error && <FieldError>{invite.error.message}</FieldError>}
              </FieldGroup>
              <DialogFooter>
                <Button type="submit" disabled={invite.isPending}>
                  {invite.isPending && <Spinner data-icon="inline-start" />}Create invitation
                </Button>
              </DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
      <Dialog open={Boolean(removeTarget)} onOpenChange={(open) => !open && setRemoveTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove member?</DialogTitle>
            <DialogDescription>
              {removeTarget?.email} will lose access to this team.
            </DialogDescription>
          </DialogHeader>
          {remove.error && <p className="text-sm text-destructive">{remove.error.message}</p>}
          <DialogFooter>
            <Button variant="outline" onClick={() => setRemoveTarget(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              disabled={remove.isPending}
              onClick={() => removeTarget && remove.mutate(removeTarget.user_id)}
            >
              Remove member
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
