import { useQuery } from "@tanstack/react-query";
import { ShieldCheckIcon } from "lucide-react";
import { Outlet } from "react-router";

import { adminApi } from "./admin.api";

export function AdminRoute() {
  const status = useQuery({
    queryKey: ["admin", "status"],
    queryFn: adminApi.status,
  });

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold">Administration</h1>
        <p className="text-sm text-muted-foreground">System-level configuration and operations.</p>
      </div>
      {status.isError ? (
        <p role="alert" className="border-l-2 border-destructive pl-3 text-sm text-destructive">
          {status.error.message}
        </p>
      ) : (
        <div className="flex min-h-24 items-center gap-4 border-y py-6">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-md bg-muted">
            <ShieldCheckIcon className="size-5" />
          </div>
          {status.data ? (
            <div>
              <p className="font-medium">{status.data.service}</p>
              <p className="text-sm text-muted-foreground">Ready mode · v{status.data.version}</p>
            </div>
          ) : (
            <div className="h-10 w-56 animate-pulse rounded-sm bg-muted" aria-hidden="true" />
          )}
        </div>
      )}

      <Outlet />
    </div>
  );
}
