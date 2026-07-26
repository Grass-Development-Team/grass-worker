import { useQuery } from "@tanstack/react-query";
import { ShieldCheckIcon } from "lucide-react";
import { NavLink, Outlet } from "react-router";

import { cn } from "@/lib/utils";

import { adminApi } from "./admin.api";

const sections = [
  { to: "/admin/nodes", label: "Nodes" },
  { to: "/admin/host-sources", label: "Host sources" },
  { to: "/admin/quota-plans", label: "Quota plans" },
  { to: "/admin/team-groups", label: "Team groups" },
  { to: "/admin/users", label: "Users" },
  { to: "/admin/teams", label: "Teams" },
  { to: "/admin/settings", label: "Settings" },
  { to: "/admin/audit", label: "Audit" },
];

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
              <p className="text-sm text-muted-foreground">
                Ready mode | Version {status.data.version}
              </p>
            </div>
          ) : (
            <div className="h-10 w-56 animate-pulse rounded-sm bg-muted" aria-hidden="true" />
          )}
        </div>
      )}

      <nav aria-label="Administration sections" className="flex flex-wrap gap-1 border-b pb-px">
        {sections.map((section) => (
          <NavLink
            key={section.to}
            to={section.to}
            className={({ isActive }) =>
              cn(
                "-mb-px border-b-2 border-transparent px-3 py-2 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground",
                isActive && "border-foreground text-foreground",
              )
            }
          >
            {section.label}
          </NavLink>
        ))}
      </nav>

      <Outlet />
    </div>
  );
}
