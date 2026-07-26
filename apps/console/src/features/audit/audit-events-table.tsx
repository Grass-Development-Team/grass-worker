import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
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

import { auditApi, type AuditEvent } from "./audit.api";

function resultBadge(result: AuditEvent["result"]) {
  switch (result) {
    case "success":
      return <Badge variant="success">Success</Badge>;
    case "denied":
      return <Badge variant="warning">Denied</Badge>;
    default:
      return <Badge variant="destructive">Failure</Badge>;
  }
}

/**
 * Shared audit event table. With `teamId` it queries the team-scoped
 * endpoint; without it the platform-admin endpoint.
 */
export function AuditEventsTable({ teamId }: { teamId?: string }) {
  const [actionFilter, setActionFilter] = useState("");

  const eventsQuery = useQuery({
    queryKey: ["audit-events", teamId ?? "admin", actionFilter],
    queryFn: () =>
      teamId
        ? auditApi.listTeam(teamId, { action: actionFilter || undefined })
        : auditApi.listAdmin({ action: actionFilter || undefined }),
  });

  return (
    <div className="space-y-4">
      <Input
        placeholder="Filter by action prefix, e.g. deployment."
        value={actionFilter}
        onChange={(event) => setActionFilter(event.target.value)}
        className="max-w-sm"
        aria-label="Filter audit events by action"
      />

      {eventsQuery.isLoading && <Skeleton className="h-64 w-full" aria-busy="true" />}
      {eventsQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {eventsQuery.error instanceof Error
            ? eventsQuery.error.message
            : "Unable to load audit events."}
        </p>
      )}
      {eventsQuery.data &&
        (eventsQuery.data.events.length === 0 ? (
          <p className="text-sm text-muted-foreground">No audit events match this filter.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Action</TableHead>
                <TableHead>Target</TableHead>
                <TableHead>Result</TableHead>
                <TableHead>Reason</TableHead>
                <TableHead>When</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {eventsQuery.data.events.map((event) => (
                <TableRow key={event.id}>
                  <TableCell className="font-mono text-xs">{event.action}</TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    <p>{event.target_type}</p>
                    {event.target_id && <p className="font-mono">{event.target_id.slice(0, 8)}</p>}
                  </TableCell>
                  <TableCell>{resultBadge(event.result)}</TableCell>
                  <TableCell className="max-w-64 truncate text-xs text-muted-foreground">
                    {event.reason ?? "—"}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {new Date(event.created_at).toLocaleString()}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}
    </div>
  );
}
