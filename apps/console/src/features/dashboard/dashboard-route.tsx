import { SettingsIcon, ShieldCheckIcon, UsersIcon } from "lucide-react";
import { Link } from "react-router";

import { Button } from "@/components/ui/button";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { useAuth } from "@/features/auth/auth-context";
import { useTeam } from "@/features/teams/team-context";

export function DashboardRoute() {
  const { user } = useAuth();
  const { activeTeam, activeRole } = useTeam();

  return (
    <div className="flex w-full flex-col gap-8">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">{activeTeam?.name ?? "Workspace"}</h1>
        <p className="text-sm text-muted-foreground">
          Team identity, membership, and access controls.
        </p>
      </div>

      <div className="grid gap-4 md:grid-cols-3">
        <Card className="gap-1 py-5">
          <CardHeader>
            <CardDescription className="text-xs font-medium tracking-wide uppercase">
              Team slug
            </CardDescription>
            <CardTitle className="text-lg">{activeTeam?.slug ?? "Unavailable"}</CardTitle>
          </CardHeader>
        </Card>
        <Card className="gap-1 py-5">
          <CardHeader>
            <CardDescription className="text-xs font-medium tracking-wide uppercase">
              Team type
            </CardDescription>
            <CardTitle className="text-lg capitalize">
              {activeTeam?.kind ?? "Unavailable"}
            </CardTitle>
          </CardHeader>
        </Card>
        <Card className="gap-1 py-5">
          <CardHeader>
            <CardDescription className="text-xs font-medium tracking-wide uppercase">
              Your access
            </CardDescription>
            <CardTitle className="text-lg capitalize">{activeRole ?? "Unavailable"}</CardTitle>
          </CardHeader>
        </Card>
      </div>

      <section className="space-y-3">
        <h2 className="text-base font-semibold">Workspace controls</h2>
        <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
          <Button asChild variant="outline" className="h-11 justify-start">
            <Link to="/settings/team">
              <SettingsIcon />
              Team settings
            </Link>
          </Button>
          <Button asChild variant="outline" className="h-11 justify-start">
            <Link to="/settings/members">
              <UsersIcon />
              Members
            </Link>
          </Button>
          {user?.platform_role === "admin" && (
            <Button asChild variant="outline" className="h-11 justify-start">
              <Link to="/admin">
                <ShieldCheckIcon />
                Administration
              </Link>
            </Button>
          )}
        </div>
      </section>
    </div>
  );
}
