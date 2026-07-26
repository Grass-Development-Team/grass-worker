import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MoreHorizontalIcon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
      setResetResult({ email: user.email, password: result.password });
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
                        <DropdownMenuSeparator />
                        <DropdownMenuItem onClick={() => resetMutation.mutate(user)}>
                          Reset password
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
    </div>
  );
}
