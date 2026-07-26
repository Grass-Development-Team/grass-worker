import { useTeam } from "@/features/teams/team-context";

import { AuditEventsTable } from "./audit-events-table";

export function TeamAuditRoute() {
  const { activeTeam } = useTeam();

  if (!activeTeam) {
    return <p className="text-sm text-muted-foreground">Select a team to view its audit trail.</p>;
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-lg font-semibold">Audit events</h1>
        <p className="text-sm text-muted-foreground">
          Key actions recorded for {activeTeam.name}: deployments, reviews, quota denials, and host
          provisioning.
        </p>
      </div>
      <AuditEventsTable teamId={activeTeam.id} />
    </div>
  );
}
