import { useQuery } from "@tanstack/react-query";
import { Navigate, Outlet, useLocation } from "react-router-dom";
import { currentUserQueryKey, getCurrentUser } from "@/api/auth";
import { Card, CardContent } from "@/components/ui/card";

export type ProtectedOutletContext = {
  currentUser: NonNullable<Awaited<ReturnType<typeof getCurrentUser>>>;
};

export function ProtectedRoute() {
  const location = useLocation();
  const { data, isPending } = useQuery({
    queryKey: currentUserQueryKey,
    queryFn: getCurrentUser,
  });

  if (isPending) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-muted/30 p-6">
        <Card className="w-full max-w-sm">
          <CardContent className="pt-6 text-sm text-muted-foreground">
            Checking session...
          </CardContent>
        </Card>
      </main>
    );
  }

  if (!data) {
    const redirect = encodeURIComponent(`${location.pathname}${location.search}`);
    return <Navigate replace to={`/login?redirect=${redirect}`} />;
  }

  return <Outlet context={{ currentUser: data }} />;
}
