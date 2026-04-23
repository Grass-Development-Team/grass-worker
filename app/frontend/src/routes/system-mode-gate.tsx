import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { Navigate, Outlet, useLocation } from "react-router-dom";
import { getSystemInfo, systemInfoQueryKey } from "@/api/system";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

function StateCard({
  title,
  description,
  action,
}: {
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <main className="flex min-h-screen items-center justify-center bg-muted/30 p-6">
      <Card className="w-full max-w-lg">
        <CardHeader>
          <CardTitle>{title}</CardTitle>
          <CardDescription>{description}</CardDescription>
        </CardHeader>
        {action ? <CardContent>{action}</CardContent> : null}
      </Card>
    </main>
  );
}

export function SystemModeGate() {
  const location = useLocation();
  const query = useQuery({
    queryKey: systemInfoQueryKey,
    queryFn: getSystemInfo,
  });

  if (query.isPending) {
    return (
      <StateCard
        title="Loading console state"
        description="Checking whether the console is still in setup mode or ready for sign-in."
      />
    );
  }

  if (query.isError) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-muted/30 p-6">
        <Card className="w-full max-w-lg">
          <CardHeader>
            <CardTitle>Unable to load console state</CardTitle>
            <CardDescription>
              The frontend could not determine whether the system is still being initialized.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <Alert variant="destructive">
              <AlertTitle>Request failed</AlertTitle>
              <AlertDescription>
                {query.error instanceof Error
                  ? query.error.message
                  : "Unable to load setup status"}
              </AlertDescription>
            </Alert>
            <Button onClick={() => void query.refetch()} type="button" variant="outline">
              Retry
            </Button>
          </CardContent>
        </Card>
      </main>
    );
  }

  if (query.data.mode === "setup" && location.pathname !== "/setup") {
    return <Navigate replace to="/setup" />;
  }

  if (query.data.mode === "ready" && location.pathname === "/setup") {
    return <Navigate replace to="/projects" />;
  }

  return <Outlet />;
}
