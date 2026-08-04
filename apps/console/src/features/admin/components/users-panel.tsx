import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MoreHorizontalIcon, PlusIcon, Trash2Icon } from "lucide-react";
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
import { Field, FieldLabel } from "@/components/ui/field";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { useAuth } from "@/features/auth/auth-context";

import { adminApi, type AdminUser } from "../admin.api";

function roleBadge(user: AdminUser) {
  return user.platform_role === "admin" ? (
    <Badge>Admin</Badge>
  ) : (
    <Badge variant="secondary">User</Badge>
  );
}

function statusBadge(user: AdminUser) {
  return user.status === "active" ? (
    <Badge variant="success">Active</Badge>
  ) : (
    <Badge variant="destructive">Disabled</Badge>
  );
}

export function UsersPanel() {
  const queryClient = useQueryClient();
  const { user: currentUser } = useAuth();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [resetResult, setResetResult] = useState<{ email: string; password: string } | null>(null);
  const [creating, setCreating] = useState(false);
  const [settingPassword, setSettingPassword] = useState<AdminUser | null>(null);
  const [renaming, setRenaming] = useState<AdminUser | null>(null);
  const [managingMfa, setManagingMfa] = useState<AdminUser | null>(null);

  const usersQuery = useQuery({
    queryKey: ["admin", "users", query],
    queryFn: () => adminApi.listUsers(query),
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "users"] });

  const updateMutation = useMutation({
    mutationFn: ({
      userId,
      input,
    }: {
      userId: string;
      input: Parameters<typeof adminApi.updateUser>[1];
    }) => adminApi.updateUser(userId, input),
    onSuccess: () => {
      setError(null);
      invalidate();
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to update the user."),
  });

  const resetMutation = useMutation({
    mutationFn: (user: AdminUser) =>
      adminApi.resetUserPassword(user.id).then((result) => ({ user, result })),
    onSuccess: ({ user, result }) => {
      setError(null);
      if (result.password) setResetResult({ email: user.email, password: result.password });
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to reset the password."),
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-4">
        <p className="text-sm text-muted-foreground">
          Every account on this platform. Disabled users cannot sign in.
        </p>
        <div className="flex items-center gap-2">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              setQuery(search);
            }}
          >
            <Input
              aria-label="Search users"
              placeholder="Search email or name…"
              className="w-64"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </form>
          <Button onClick={() => setCreating(true)}>
            <PlusIcon /> New user
          </Button>
        </div>
      </div>

      {resetResult && (
        <div className="rounded-md border bg-muted/40 p-3 text-sm">
          <p className="font-medium">Temporary password for {resetResult.email}</p>
          <p className="text-muted-foreground">
            Share it securely — it is shown only once and stored hashed.
          </p>
          <code className="mt-1 block break-all rounded bg-background p-2 text-xs">
            {resetResult.password}
          </code>
        </div>
      )}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      {usersQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {usersQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {usersQuery.error instanceof Error ? usersQuery.error.message : "Unable to load users."}
        </p>
      )}
      {usersQuery.data && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>User</TableHead>
              <TableHead>Role</TableHead>
              <TableHead>Status</TableHead>
              <TableHead>Last login</TableHead>
              <TableHead>Created</TableHead>
              <TableHead className="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {usersQuery.data.users.map((user) => {
              const isSelf = user.id === currentUser?.id;
              return (
                <TableRow key={user.id}>
                  <TableCell>
                    <span className="font-medium">{user.display_name ?? user.email}</span>
                    <p className="text-xs text-muted-foreground">{user.email}</p>
                  </TableCell>
                  <TableCell>{roleBadge(user)}</TableCell>
                  <TableCell>{statusBadge(user)}</TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {user.last_login_at ? new Date(user.last_login_at).toLocaleString() : "Never"}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(user.created_at).toLocaleDateString()}
                  </TableCell>
                  <TableCell className="text-right">
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button size="sm" variant="ghost" aria-label={`Actions for ${user.email}`}>
                          <MoreHorizontalIcon />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuLabel>{user.email}</DropdownMenuLabel>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem
                          disabled={isSelf}
                          onClick={() =>
                            updateMutation.mutate({
                              userId: user.id,
                              input: {
                                platform_role: user.platform_role === "admin" ? "user" : "admin",
                              },
                            })
                          }
                        >
                          {user.platform_role === "admin"
                            ? "Remove platform admin"
                            : "Make platform admin"}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          disabled={isSelf}
                          onClick={() =>
                            updateMutation.mutate({
                              userId: user.id,
                              input: { status: user.status === "active" ? "disabled" : "active" },
                            })
                          }
                        >
                          {user.status === "active" ? "Disable account" : "Enable account"}
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => setRenaming(user)}>
                          Edit display name
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem onClick={() => setSettingPassword(user)}>
                          Set password…
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => resetMutation.mutate(user)}>
                          Generate new password
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => setManagingMfa(user)}>
                          Manage MFA
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}

      {creating && (
        <CreateUserDialog
          onClose={() => setCreating(false)}
          onCreated={(email, password) => {
            setCreating(false);
            if (password) setResetResult({ email, password });
            invalidate();
          }}
        />
      )}
      {settingPassword && (
        <SetPasswordDialog
          user={settingPassword}
          onClose={() => setSettingPassword(null)}
          onSaved={() => setSettingPassword(null)}
        />
      )}
      {renaming && (
        <RenameUserDialog
          user={renaming}
          onClose={() => setRenaming(null)}
          onSaved={() => {
            setRenaming(null);
            invalidate();
          }}
        />
      )}
      {managingMfa && <MfaFactorsDialog user={managingMfa} onClose={() => setManagingMfa(null)} />}
    </div>
  );
}

function MfaFactorsDialog({ user, onClose }: { user: AdminUser; onClose: () => void }) {
  const queryClient = useQueryClient();
  const factors = useQuery({
    queryKey: ["admin", "users", user.id, "mfa"],
    queryFn: () => adminApi.listUserMfa(user.id),
  });
  const reset = useMutation({
    mutationFn: (factorId: string) => adminApi.resetUserMfaFactor(user.id, factorId),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["admin", "users", user.id, "mfa"] }),
  });
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Multi-factor authentication</DialogTitle>
          <DialogDescription>{user.email}</DialogDescription>
        </DialogHeader>
        {factors.isLoading && <Skeleton className="h-24 w-full" aria-busy="true" />}
        {factors.isError && (
          <p role="alert" className="text-sm text-destructive">
            {factors.error instanceof Error ? factors.error.message : "Unable to load factors."}
          </p>
        )}
        {factors.data?.factors.length === 0 && (
          <p className="text-sm text-muted-foreground">No factors enrolled.</p>
        )}
        <div className="grid gap-2">
          {factors.data?.factors.map((factor) => (
            <div
              key={factor.id}
              className="flex items-center justify-between gap-4 rounded-md border p-3"
            >
              <div>
                <p className="text-sm font-medium">
                  {factor.kind === "totp" ? "Authenticator app" : "Email code"}
                </p>
                <p className="text-xs text-muted-foreground">
                  {factor.verified ? "Verified" : "Pending"}
                  {factor.last_used_at
                    ? ` · Last used ${new Date(factor.last_used_at).toLocaleString()}`
                    : ""}
                </p>
              </div>
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                aria-label={`Reset ${factor.kind} factor`}
                disabled={reset.isPending}
                onClick={() => reset.mutate(factor.id)}
              >
                <Trash2Icon />
              </Button>
            </div>
          ))}
        </div>
        {reset.isError && (
          <p role="alert" className="text-sm text-destructive">
            {reset.error instanceof Error ? reset.error.message : "Unable to reset the factor."}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}

function CreateUserDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (email: string, password: string | null) => void;
}) {
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [role, setRole] = useState<"user" | "admin">("user");
  const [password, setPassword] = useState("");

  const mutation = useMutation({
    mutationFn: () =>
      adminApi.createUser({
        email,
        display_name: displayName.trim() || undefined,
        platform_role: role,
        password: password || undefined,
      }),
    onSuccess: (result) => onCreated(result.user.email, result.password),
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New user</DialogTitle>
          <DialogDescription>
            Creates an account with its personal team, bypassing the signup policy.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (email.trim()) mutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="new-user-email">Email</FieldLabel>
            <Input
              id="new-user-email"
              type="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="new-user-name">Display name</FieldLabel>
            <Input
              id="new-user-name"
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
            />
          </Field>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="new-user-role">Platform role</FieldLabel>
              <Select value={role} onValueChange={(value) => setRole(value as typeof role)}>
                <SelectTrigger id="new-user-role">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="user">User</SelectItem>
                  <SelectItem value="admin">Platform admin</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel htmlFor="new-user-password">Password</FieldLabel>
              <Input
                id="new-user-password"
                type="password"
                placeholder="Leave empty to generate"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </Field>
          </div>
          {mutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : "Unable to create."}
            </p>
          )}
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Creating…" : "Create user"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function SetPasswordDialog({
  user,
  onClose,
  onSaved,
}: {
  user: AdminUser;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [mismatch, setMismatch] = useState(false);

  const mutation = useMutation({
    mutationFn: () => adminApi.resetUserPassword(user.id, password),
    onSuccess: onSaved,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Set password</DialogTitle>
          <DialogDescription>{user.email} — the password is never displayed.</DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (password !== confirm) {
              setMismatch(true);
              return;
            }
            setMismatch(false);
            mutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="set-password">New password</FieldLabel>
            <Input
              id="set-password"
              type="password"
              minLength={8}
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="set-password-confirm">Confirm password</FieldLabel>
            <Input
              id="set-password-confirm"
              type="password"
              value={confirm}
              onChange={(event) => setConfirm(event.target.value)}
              required
            />
          </Field>
          {mismatch && (
            <p role="alert" className="text-sm text-destructive">
              Passwords do not match.
            </p>
          )}
          {mutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : "Unable to save."}
            </p>
          )}
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Saving…" : "Set password"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function RenameUserDialog({
  user,
  onClose,
  onSaved,
}: {
  user: AdminUser;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [displayName, setDisplayName] = useState(user.display_name ?? "");

  const mutation = useMutation({
    mutationFn: () => adminApi.updateUser(user.id, { display_name: displayName.trim() || null }),
    onSuccess: onSaved,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Edit display name</DialogTitle>
          <DialogDescription>{user.email}</DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            mutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="rename-user">Display name</FieldLabel>
            <Input
              id="rename-user"
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              placeholder="Empty clears the name"
            />
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
