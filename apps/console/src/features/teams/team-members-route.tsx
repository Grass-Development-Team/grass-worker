import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckIcon,
  CopyIcon,
  MoreHorizontalIcon,
  PlusIcon,
  UserIcon,
  UserPlusIcon,
} from "lucide-react";
import { useDeferredValue, useState } from "react";

import { Badge } from "@/components/ui/badge";
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
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
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
import {
  teamsApi,
  type InvitationCandidate,
  type ManagedTeamRole,
  type TeamInvitation,
  type TeamMember,
} from "./teams.api";

const roles: ManagedTeamRole[] = ["admin", "member", "viewer"];

export function TeamMembersRoute() {
  const { activeTeam, activeRole } = useTeam();
  const queryClient = useQueryClient();
  const [inviteOpen, setInviteOpen] = useState(false);
  const [email, setEmail] = useState("");
  const [selectedCandidate, setSelectedCandidate] = useState<InvitationCandidate | null>(null);
  const [role, setRole] = useState<ManagedTeamRole>("member");
  const [invitation, setInvitation] = useState<TeamInvitation | null>(null);
  const [removeTarget, setRemoveTarget] = useState<TeamMember | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied">("idle");
  const [copyError, setCopyError] = useState<string | null>(null);
  const manageable = activeRole ? canManageMembers(activeRole) : false;
  const members = useQuery({
    queryKey: activeTeam ? teamKeys.members(activeTeam.id) : ["teams", "none", "members"],
    queryFn: () => teamsApi.listMembers(activeTeam!.id),
    enabled: Boolean(activeTeam),
  });
  const candidateQuery = useDeferredValue(email.trim());
  const candidates = useQuery({
    queryKey: activeTeam
      ? ["teams", activeTeam.id, "invitation-candidates", candidateQuery]
      : ["teams", "none", "invitation-candidates", candidateQuery],
    queryFn: () => teamsApi.invitationCandidates(activeTeam!.id, candidateQuery),
    enabled: Boolean(activeTeam && inviteOpen && !invitation && candidateQuery),
  });
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: teamKeys.members(activeTeam!.id) });
  const invite = useMutation({
    mutationFn: () =>
      teamsApi.inviteMember(activeTeam!.id, { email: selectedCandidate!.email, role }),
    onSuccess: ({ invitation }) => {
      setInvitation(invitation);
      setCopyError(null);
      setCopyState("idle");
    },
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

  const setInviteDialogOpen = (open: boolean) => {
    setInviteOpen(open);
    if (!open) {
      setEmail("");
      setSelectedCandidate(null);
      setRole("member");
      setInvitation(null);
      setCopyError(null);
      setCopyState("idle");
      invite.reset();
    }
  };

  const openInviteDialog = () => {
    invite.reset();
    setEmail("");
    setSelectedCandidate(null);
    setRole("member");
    setInvitation(null);
    setCopyError(null);
    setCopyState("idle");
    setInviteOpen(true);
  };

  const setRemoveDialogOpen = (open: boolean) => {
    if (!open) {
      setRemoveTarget(null);
      remove.reset();
    }
  };

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <div className="flex items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">Members</h1>
          <p className="text-sm text-muted-foreground">Manage access to {activeTeam?.name}.</p>
        </div>
        {manageable && (
          <Button onClick={openInviteDialog}>
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
            <p role="alert" className="text-sm text-destructive">
              {members.error.message}
            </p>
          ) : members.data?.members.length === 0 ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              No members found for this team.
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="px-0 pr-2 md:px-4">User</TableHead>
                  <TableHead className="px-2 md:px-4">Role</TableHead>
                  <TableHead className="hidden md:table-cell">Joined</TableHead>
                  <TableHead className="w-10 px-0 md:px-4">
                    <span className="sr-only">Actions</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {members.data?.members.map((member) => (
                  <TableRow key={member.id}>
                    <TableCell className="px-0 pr-2 md:px-4">
                      <div className="font-medium">{member.display_name || member.email}</div>
                      <div className="text-xs text-muted-foreground">{member.email}</div>
                    </TableCell>
                    <TableCell className="px-2 md:px-4">
                      {member.role === "owner" || !manageable ? (
                        <span className="capitalize">{member.role}</span>
                      ) : (
                        <Select
                          value={member.role}
                          disabled={updateRole.isPending}
                          onValueChange={(value) => {
                            updateRole.reset();
                            updateRole.mutate({
                              userId: member.user_id,
                              role: value as ManagedTeamRole,
                            });
                          }}
                        >
                          <SelectTrigger
                            aria-label={`Role for ${member.email}`}
                            className="w-28 md:w-32"
                          >
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
                    <TableCell className="hidden md:table-cell">
                      {new Date(member.joined_at).toLocaleDateString()}
                    </TableCell>
                    <TableCell className="w-10 px-0 text-right md:px-4">
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
                                onSelect={() =>
                                  setTimeout(() => {
                                    remove.reset();
                                    setRemoveTarget(member);
                                  }, 0)
                                }
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
          {updateRole.error && (
            <p role="alert" className="mt-4 text-sm text-destructive">
              {updateRole.error.message}
            </p>
          )}
        </CardContent>
      </Card>
      <Dialog open={inviteOpen} onOpenChange={setInviteDialogOpen}>
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
            <div className="flex flex-col gap-2">
              <div className="flex gap-2">
                <Input value={link} readOnly aria-label="Invitation link" />
                <Button
                  type="button"
                  size="icon"
                  onClick={async () => {
                    try {
                      await navigator.clipboard.writeText(link);
                      setCopyError(null);
                      setCopyState("copied");
                    } catch {
                      setCopyError("Unable to copy the invitation link.");
                    }
                  }}
                  aria-label={
                    copyState === "copied" ? "Invitation link copied" : "Copy invitation link"
                  }
                >
                  {copyState === "copied" ? <CheckIcon /> : <CopyIcon />}
                </Button>
              </div>
              {copyError && (
                <p role="alert" className="text-sm text-destructive">
                  {copyError}
                </p>
              )}
            </div>
          ) : (
            <form
              className="flex flex-col gap-6"
              onSubmit={(event) => {
                event.preventDefault();
                if (selectedCandidate) invite.mutate();
              }}
            >
              <FieldGroup>
                <Field>
                  <FieldLabel htmlFor="invite-email">Email</FieldLabel>
                  <Command shouldFilter={false} className="border">
                    <CommandInput
                      id="invite-email"
                      aria-label="Email"
                      placeholder="Search by email or name"
                      value={email}
                      onValueChange={(value) => {
                        setEmail(value);
                        setSelectedCandidate(null);
                      }}
                    />
                    {candidateQuery && (
                      <CommandList>
                        {candidates.isFetching ? (
                          <CommandGroup>
                            <CommandItem disabled value="searching">
                              <Spinner /> Searching...
                            </CommandItem>
                          </CommandGroup>
                        ) : candidates.error ? null : (
                          <>
                            <CommandEmpty>User does not exist.</CommandEmpty>
                            {candidates.data?.candidates.length ? (
                              <CommandGroup>
                                {candidates.data.candidates.map((candidate) => (
                                  <CommandItem
                                    key={`${candidate.kind}:${candidate.user_id ?? candidate.email}`}
                                    value={`${candidate.kind}:${candidate.user_id ?? candidate.email}`}
                                    onSelect={() => {
                                      setEmail(candidate.email);
                                      setSelectedCandidate(candidate);
                                    }}
                                  >
                                    {candidate.kind === "user" ? <UserIcon /> : <UserPlusIcon />}
                                    <span className="flex min-w-0 flex-1 flex-col">
                                      <span className="truncate font-medium">
                                        {candidate.display_name ?? candidate.email}
                                      </span>
                                      {candidate.display_name && (
                                        <span className="truncate text-xs text-muted-foreground">
                                          {candidate.email}
                                        </span>
                                      )}
                                    </span>
                                    {candidate.kind === "email" && (
                                      <Badge variant="secondary">Invite User</Badge>
                                    )}
                                  </CommandItem>
                                ))}
                              </CommandGroup>
                            ) : null}
                          </>
                        )}
                      </CommandList>
                    )}
                  </Command>
                  {candidates.error && <FieldError>{candidates.error.message}</FieldError>}
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
                <Button type="submit" disabled={invite.isPending || !selectedCandidate}>
                  {invite.isPending && <Spinner data-icon="inline-start" />}Create invitation
                </Button>
              </DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>
      <Dialog open={Boolean(removeTarget)} onOpenChange={setRemoveDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove member?</DialogTitle>
            <DialogDescription>
              {removeTarget?.email} will lose access to this team.
            </DialogDescription>
          </DialogHeader>
          {remove.error && (
            <p role="alert" className="text-sm text-destructive">
              {remove.error.message}
            </p>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setRemoveDialogOpen(false)}>
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
