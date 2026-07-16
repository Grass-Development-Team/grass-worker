import { SettingsIcon, ShieldCheckIcon, UsersIcon } from "lucide-react";
import { Link } from "react-router";

import { Button } from "@/components/ui/button";
import { useTeam } from "@/features/teams/team-context";

export function DashboardRoute() {
  const { activeTeam, activeRole } = useTeam();

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-8">
      <div>
        <h1 className="text-2xl font-semibold">{activeTeam?.name ?? "Workspace"}</h1>
        <p className="text-sm text-muted-foreground">
          Team identity, membership, and access controls.
        </p>
      </div>

      <div className="grid border-y md:grid-cols-3">
        <div className="flex min-h-28 flex-col justify-center gap-1 border-b py-5 md:border-b-0 md:border-r md:px-5">
          <span className="text-xs font-medium uppercase text-muted-foreground">Team slug</span>
          <span className="font-medium">{activeTeam?.slug ?? "Unavailable"}</span>
        </div>
        <div className="flex min-h-28 flex-col justify-center gap-1 border-b py-5 md:border-b-0 md:border-r md:px-5">
          <span className="text-xs font-medium uppercase text-muted-foreground">Team type</span>
          <span className="font-medium capitalize">{activeTeam?.kind ?? "Unavailable"}</span>
        </div>
        <div className="flex min-h-28 flex-col justify-center gap-1 py-5 md:px-5">
          <span className="text-xs font-medium uppercase text-muted-foreground">Your access</span>
          <span className="font-medium capitalize">{activeRole ?? "Unavailable"}</span>
        </div>
      </div>

      <section className="space-y-3">
        <h2 className="text-base font-semibold">Workspace controls</h2>
        <div className="grid gap-2 sm:grid-cols-3">
          <Button asChild variant="outline" className="h-12 justify-start">
            <Link to="/settings/team">
              <SettingsIcon data-icon="inline-start" />
              Team settings
            </Link>
          </Button>
          <Button asChild variant="outline" className="h-12 justify-start">
            <Link to="/settings/members">
              <UsersIcon data-icon="inline-start" />
              Members
            </Link>
          </Button>
          <Button asChild variant="outline" className="h-12 justify-start">
            <Link to="/admin">
              <ShieldCheckIcon data-icon="inline-start" />
              Administration
            </Link>
          </Button>
        </div>
      </section>
    </div>
  );
}
