import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useOutletContext } from "react-router-dom";
import { currentUserQueryKey, logout } from "@/api/auth";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { ProtectedOutletContext } from "./protected-route";

export function HomePage() {
  const { currentUser } = useOutletContext<ProtectedOutletContext>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const mutation = useMutation({
    mutationFn: logout,
    onSuccess: async () => {
      queryClient.setQueryData(currentUserQueryKey, null);
      await navigate("/login?redirect=%2F", { replace: true });
    },
  });

  return (
    <main className="min-h-screen bg-muted/30 px-6 py-10">
      <div className="mx-auto grid max-w-5xl gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(280px,1fr)]">
        <Card>
          <CardHeader>
              <CardTitle>Ready mode is active</CardTitle>
              <CardDescription>
                The initial administrator session is valid and protected routes
                are now available.
              </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="rounded-lg border bg-background p-4">
                <p className="text-sm text-muted-foreground">Email</p>
                <p className="mt-1 font-medium">{currentUser.email}</p>
              </div>
              <div className="rounded-lg border bg-background p-4">
                <p className="text-sm text-muted-foreground">Role</p>
                <p className="mt-1 font-medium">
                  {currentUser.is_admin ? "Administrator" : "User"}
                </p>
              </div>
            </div>
            <div className="rounded-lg border bg-background p-4 text-sm text-muted-foreground">
              {currentUser.is_initial_admin
                ? "This session belongs to the initial administrator created during setup."
                : "This session belongs to a signed-in user."}
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Session</CardTitle>
            <CardDescription>
              Sign out to clear the current `HttpOnly` session cookie.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <Button
              className="w-full"
              disabled={mutation.isPending}
              onClick={() => mutation.mutate()}
              variant="outline"
            >
              {mutation.isPending ? "Signing out..." : "Sign out"}
            </Button>
          </CardContent>
        </Card>
      </div>
    </main>
  );
}
