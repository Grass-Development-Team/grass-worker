import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MoreHorizontalIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useEffect, useState } from "react";

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

import { adminApi, type AdminUser, type AdminUserMfaPolicy } from "../admin.api";

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
  const [selectedUserIds, setSelectedUserIds] = useState<Set<string>>(new Set());

  const usersQuery = useQuery({
    queryKey: ["admin", "users", query],
    queryFn: () => adminApi.listUsers(query),
  });
  const visibleUserIds = usersQuery.data?.users.map((user) => user.id) ?? [];
  const selectedVisibleCount = visibleUserIds.filter((id) => selectedUserIds.has(id)).length;
  const allVisibleSelected =
    visibleUserIds.length > 0 && selectedVisibleCount === visibleUserIds.length;

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
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <p className="text-sm text-muted-foreground">
          Every account on this platform. Disabled users cannot sign in.
        </p>
        <div className="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              setQuery(search);
              setSelectedUserIds(new Set());
            }}
          >
            <Input
              aria-label="Search users"
              placeholder="Search email or name…"
              className="w-full sm:w-64"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </form>
          <Button onClick={() => setCreating(true)}>
            <PlusIcon /> New user
          </Button>
        </div>
      </div>

      {selectedUserIds.size > 0 && (
        <div className="flex min-h-10 items-center justify-between gap-4 border-y px-1 py-2">
          <p className="text-sm font-medium">{selectedUserIds.size} users selected</p>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => setSelectedUserIds(new Set())}
          >
            Clear selection
          </Button>
        </div>
      )}

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
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-10">
                  <Checkbox
                    aria-label="Select all visible users"
                    checked={
                      allVisibleSelected ? true : selectedVisibleCount > 0 ? "indeterminate" : false
                    }
                    onCheckedChange={(checked) =>
                      setSelectedUserIds((current) => {
                        const next = new Set(current);
                        for (const id of visibleUserIds) {
                          if (checked === true) next.add(id);
                          else next.delete(id);
                        }
                        return next;
                      })
                    }
                  />
                </TableHead>
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
                      <Checkbox
                        aria-label={`Select ${user.email}`}
                        checked={selectedUserIds.has(user.id)}
                        onCheckedChange={(checked) =>
                          setSelectedUserIds((current) => {
                            const next = new Set(current);
                            if (checked === true) next.add(user.id);
                            else next.delete(user.id);
                            return next;
                          })
                        }
                      />
                    </TableCell>
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
                          <Button
                            size="sm"
                            variant="ghost"
                            aria-label={`Actions for ${user.email}`}
                          >
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
        </div>
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
  const [policy, setPolicy] = useState<AdminUserMfaPolicy | null>(null);
  const factors = useQuery({
    queryKey: ["admin", "users", user.id, "mfa"],
    queryFn: () => adminApi.listUserMfa(user.id),
  });
  useEffect(() => {
    if (factors.data) setPolicy(factors.data.policy);
  }, [factors.data]);
  const reset = useMutation({
    mutationFn: (factorId: string) => adminApi.resetUserMfaFactor(user.id, factorId),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["admin", "users", user.id, "mfa"] }),
  });
  const updatePolicy = useMutation({
    mutationFn: (nextPolicy: AdminUserMfaPolicy) =>
      adminApi.updateUserMfaPolicy(user.id, nextPolicy),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["admin", "users", user.id, "mfa"] }),
  });
  const formatTimestamp = (value: string | null | undefined, fallback: string) => {
    if (!value) return fallback;
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? "Unknown date" : date.toLocaleString();
  };
  const toggleRequired = (factor: "totp" | "email", checked: boolean) =>
    setPolicy((current) =>
      current
        ? {
            ...current,
            required_factors: checked
              ? [...new Set([...current.required_factors, factor])]
              : current.required_factors.filter((item) => item !== factor),
          }
        : current,
    );
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-4xl">
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
        {factors.data && policy && (
          <div className="grid gap-6">
            <section className="grid gap-3">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <h3 className="text-sm font-medium">Enforcement policy</h3>
                  <p className="text-xs text-muted-foreground">
                    Custom requirements can strengthen, but cannot weaken, the platform baseline.
                  </p>
                </div>
                <Badge variant="secondary">
                  Effective minimum: {factors.data.effective_requirements.minimum_factors}
                </Badge>
              </div>
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={!policy.inherit_platform}
                  onCheckedChange={(checked) =>
                    setPolicy((current) =>
                      current
                        ? {
                            ...current,
                            inherit_platform: checked !== true,
                            ...(checked === true
                              ? {}
                              : { minimum_factors: 0, required_factors: [] }),
                          }
                        : current,
                    )
                  }
                />
                Use a custom policy for this user
              </label>
              {!policy.inherit_platform && (
                <div className="grid gap-4 rounded-md border p-4 sm:grid-cols-2">
                  <Field>
                    <FieldLabel htmlFor={`user-mfa-minimum-${user.id}`}>
                      Minimum enrolled methods
                    </FieldLabel>
                    <Input
                      id={`user-mfa-minimum-${user.id}`}
                      type="number"
                      min={0}
                      max={factors.data.allowed_factors.length}
                      value={policy.minimum_factors}
                      onChange={(event) =>
                        setPolicy((current) =>
                          current
                            ? {
                                ...current,
                                minimum_factors: Math.max(
                                  0,
                                  Math.min(
                                    factors.data.allowed_factors.length,
                                    Number(event.target.value),
                                  ),
                                ),
                              }
                            : current,
                        )
                      }
                    />
                  </Field>
                  <Field>
                    <FieldLabel>Required methods</FieldLabel>
                    <div className="flex min-h-9 flex-wrap items-center gap-4">
                      {factors.data.allowed_factors.map((factor) => (
                        <label key={factor} className="flex items-center gap-2 text-sm">
                          <Checkbox
                            checked={policy.required_factors.includes(factor)}
                            onCheckedChange={(checked) => toggleRequired(factor, checked === true)}
                          />
                          {factor === "totp" ? "Authenticator app" : "Email code"}
                        </label>
                      ))}
                    </div>
                  </Field>
                </div>
              )}
              <div className="flex justify-end">
                <Button
                  type="button"
                  size="sm"
                  onClick={() => updatePolicy.mutate(policy)}
                  disabled={updatePolicy.isPending}
                >
                  {updatePolicy.isPending ? "Saving..." : "Save policy"}
                </Button>
              </div>
            </section>
            <section className="grid gap-3">
              <div>
                <h3 className="text-sm font-medium">Enrolled methods</h3>
                <p className="text-xs text-muted-foreground">
                  Resetting a method requires the user to enroll it again.
                </p>
              </div>
              <div className="overflow-x-auto rounded-md border">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Method</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead>Added</TableHead>
                      <TableHead>Last used</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {factors.data.factors.length === 0 ? (
                      <TableRow>
                        <TableCell
                          colSpan={5}
                          className="h-20 text-center text-sm text-muted-foreground"
                        >
                          No factors enrolled.
                        </TableCell>
                      </TableRow>
                    ) : (
                      factors.data.factors.map((factor) => (
                        <TableRow key={factor.id}>
                          <TableCell className="font-medium">
                            {factor.kind === "totp" ? "Authenticator app" : "Email code"}
                          </TableCell>
                          <TableCell>
                            <Badge variant={factor.verified ? "success" : "secondary"}>
                              {factor.verified ? "Verified" : "Pending"}
                            </Badge>
                          </TableCell>
                          <TableCell className="text-sm text-muted-foreground">
                            {formatTimestamp(factor.created_at, "Unknown date")}
                          </TableCell>
                          <TableCell className="text-sm text-muted-foreground">
                            {formatTimestamp(factor.last_used_at, "Never")}
                          </TableCell>
                          <TableCell className="text-right">
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
                          </TableCell>
                        </TableRow>
                      ))
                    )}
                  </TableBody>
                </Table>
              </div>
            </section>
          </div>
        )}
        {reset.isError && (
          <p role="alert" className="text-sm text-destructive">
            {reset.error instanceof Error ? reset.error.message : "Unable to reset the factor."}
          </p>
        )}
        {updatePolicy.isError && (
          <p role="alert" className="text-sm text-destructive">
            {updatePolicy.error instanceof Error
              ? updatePolicy.error.message
              : "Unable to save the MFA policy."}
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
