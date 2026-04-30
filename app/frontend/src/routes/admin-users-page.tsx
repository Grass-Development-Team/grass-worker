import { useQuery } from "@tanstack/react-query";
import { useOutletContext } from "react-router-dom";
import { adminUsersQueryKey, getAdminUsers, type User } from "@/api/users";
import { ConsolePageHeader } from "@/components/console/console-page-header";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import type { ProtectedOutletContext } from "./protected-route";

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) {
    return error.message;
  }

  return fallback;
}

function roleBadges(user: User) {
  if (user.is_initial_admin) {
    return (
      <>
        <Badge>Initial admin</Badge>
        <Badge variant="secondary">Administrator</Badge>
      </>
    );
  }

  if (user.is_admin) {
    return <Badge>Administrator</Badge>;
  }

  return <Badge variant="outline">Member</Badge>;
}

export function AdminUsersPage() {
  const { currentUser } = useOutletContext<ProtectedOutletContext>();
  const usersQuery = useQuery({
    queryKey: adminUsersQueryKey,
    queryFn: getAdminUsers,
  });

  return (
    <div className="space-y-6">
      <ConsolePageHeader
        actions={
          <Button
            disabled={usersQuery.isPending}
            onClick={() => void usersQuery.refetch()}
            type="button"
            variant="outline"
          >
            {usersQuery.isPending ? "Refreshing..." : "Refresh users"}
          </Button>
        }
        description={`Review the user inventory and admin assignments visible to ${currentUser.email}.`}
        eyebrow="Admin"
        title="Users"
      />

      {usersQuery.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Unable to load users</AlertTitle>
          <AlertDescription>
            {errorMessage(usersQuery.error, "The user inventory request failed.")}
          </AlertDescription>
        </Alert>
      ) : null}

      <Card>
        <CardHeader>
          <CardTitle>User inventory</CardTitle>
          <CardDescription>
            Track administrator access and member accounts provisioned in this control plane.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {usersQuery.isPending ? (
            <div className="space-y-3">
              <Skeleton className="h-24" />
              <Skeleton className="h-24" />
            </div>
          ) : usersQuery.data?.length ? (
            usersQuery.data.map((user) => (
              <Card key={user.id}>
                <CardHeader>
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div className="space-y-1">
                      <CardTitle>
                        <h2>{user.email}</h2>
                      </CardTitle>
                      <CardDescription>{user.id}</CardDescription>
                    </div>
                    <div className="flex flex-wrap gap-2">{roleBadges(user)}</div>
                  </div>
                </CardHeader>
                <CardContent className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-2">
                  <div className="space-y-1">
                    <p>Created</p>
                    <p className="font-medium text-foreground">
                      {formatTimestamp(user.created_at)}
                    </p>
                  </div>
                  <div className="space-y-1">
                    <p>Updated</p>
                    <p className="font-medium text-foreground">
                      {formatTimestamp(user.updated_at)}
                    </p>
                  </div>
                </CardContent>
              </Card>
            ))
          ) : (
            <Card>
              <CardHeader>
                <CardTitle>No users found</CardTitle>
                <CardDescription>
                  The control plane did not return any user records for this environment.
                </CardDescription>
              </CardHeader>
            </Card>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
