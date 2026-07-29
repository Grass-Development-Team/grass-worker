import { useQuery } from "@tanstack/react-query";
import { ArrowLeftIcon, ArrowRightIcon, EyeIcon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { auditApi, type AuditActorType, type AuditEvent, type AuditResult } from "./audit.api";

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

function actorLabel(event: AuditEvent) {
  if (event.actor_user_id) return `User ${event.actor_user_id.slice(0, 8)}`;
  if (event.actor_node_id) return `Node ${event.actor_node_id.slice(0, 8)}`;
  return event.actor_type === "anonymous" ? "Anonymous" : "System";
}

function JsonBlock({ value }: { value: Record<string, unknown> }) {
  return (
    <pre className="overflow-x-auto rounded-md bg-muted p-3 font-mono text-xs">
      {JSON.stringify(value, null, 2)}
    </pre>
  );
}

function EventDetails({ event }: { event: AuditEvent }) {
  const details = [
    ["Time", new Date(event.created_at).toLocaleString()],
    ["Actor", actorLabel(event)],
    ["Target", `${event.target_type}${event.target_id ? ` · ${event.target_id}` : ""}`],
    ["Request ID", event.request_id ?? "Not recorded"],
    ["Source IP", event.source_ip ?? "Not recorded"],
    [
      "Request",
      event.http_method && event.request_path
        ? `${event.http_method} ${event.request_path}`
        : "Not recorded",
    ],
    ["Status", event.status_code === null ? "Not recorded" : String(event.status_code)],
    ["Duration", event.duration_ms === null ? "Not recorded" : `${event.duration_ms} ms`],
    ["User agent", event.user_agent ?? "Not recorded"],
    ["Reason", event.reason ?? "None"],
  ];

  return (
    <div className="flex flex-col gap-5 overflow-y-auto px-4 pb-4">
      <dl className="grid gap-3 text-sm sm:grid-cols-[7rem_1fr]">
        {details.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="min-w-0 break-words font-mono text-xs">{value}</dd>
          </div>
        ))}
      </dl>
      <section className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Changes</h3>
        <JsonBlock value={event.changes} />
      </section>
      <section className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Metadata</h3>
        <JsonBlock value={event.metadata} />
      </section>
    </div>
  );
}

export function AuditEventsTable({ teamId }: { teamId?: string }) {
  const [action, setAction] = useState("");
  const [actorUserId, setActorUserId] = useState("");
  const [actorType, setActorType] = useState<AuditActorType | "all">("all");
  const [targetType, setTargetType] = useState("");
  const [targetId, setTargetId] = useState("");
  const [result, setResult] = useState<AuditResult | "all">("all");
  const [createdFrom, setCreatedFrom] = useState("");
  const [createdTo, setCreatedTo] = useState("");
  const [page, setPage] = useState(1);
  const [selected, setSelected] = useState<AuditEvent | null>(null);

  const filters = {
    action: action.trim() || undefined,
    actor_user_id: actorUserId.trim() || undefined,
    actor_type: actorType === "all" ? undefined : actorType,
    target_type: targetType.trim() || undefined,
    target_id: targetId.trim() || undefined,
    result: result === "all" ? undefined : result,
    from: createdFrom ? new Date(createdFrom).getTime() : undefined,
    to: createdTo ? new Date(createdTo).getTime() : undefined,
    page,
    per_page: 50,
  };
  const eventsQuery = useQuery({
    queryKey: ["audit-events", teamId ?? "admin", filters],
    queryFn: () => (teamId ? auditApi.listTeam(teamId, filters) : auditApi.listAdmin(filters)),
  });
  const pagination = eventsQuery.data?.pagination;

  function updateFilter(update: () => void) {
    setPage(1);
    update();
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
        <Input
          placeholder="Action prefix"
          value={action}
          onChange={(event) => updateFilter(() => setAction(event.target.value))}
          aria-label="Filter audit events by action"
        />
        <Input
          placeholder="Actor user ID"
          value={actorUserId}
          onChange={(event) => updateFilter(() => setActorUserId(event.target.value))}
          aria-label="Filter audit events by actor user ID"
        />
        <Select
          value={actorType}
          onValueChange={(value) =>
            updateFilter(() => setActorType(value as AuditActorType | "all"))
          }
        >
          <SelectTrigger aria-label="Filter audit events by actor type">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="all">All actor types</SelectItem>
              <SelectItem value="user">User</SelectItem>
              <SelectItem value="node">Node</SelectItem>
              <SelectItem value="system">System</SelectItem>
              <SelectItem value="anonymous">Anonymous</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
        <Input
          placeholder="Target type"
          value={targetType}
          onChange={(event) => updateFilter(() => setTargetType(event.target.value))}
          aria-label="Filter audit events by target type"
        />
        <Input
          placeholder="Target ID"
          value={targetId}
          onChange={(event) => updateFilter(() => setTargetId(event.target.value))}
          aria-label="Filter audit events by target ID"
        />
        <Select
          value={result}
          onValueChange={(value) => updateFilter(() => setResult(value as AuditResult | "all"))}
        >
          <SelectTrigger aria-label="Filter audit events by result">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="all">All results</SelectItem>
              <SelectItem value="success">Success</SelectItem>
              <SelectItem value="failure">Failure</SelectItem>
              <SelectItem value="denied">Denied</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
        <Input
          type="datetime-local"
          value={createdFrom}
          onChange={(event) => updateFilter(() => setCreatedFrom(event.target.value))}
          aria-label="Filter audit events from time"
        />
        <Input
          type="datetime-local"
          value={createdTo}
          onChange={(event) => updateFilter(() => setCreatedTo(event.target.value))}
          aria-label="Filter audit events to time"
        />
      </div>

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
                <TableHead>Time</TableHead>
                <TableHead>Actor</TableHead>
                <TableHead>Action</TableHead>
                <TableHead>Target</TableHead>
                <TableHead>Result</TableHead>
                <TableHead className="w-12">
                  <span className="sr-only">Details</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {eventsQuery.data.events.map((event) => (
                <TableRow key={event.id}>
                  <TableCell className="whitespace-nowrap text-xs text-muted-foreground">
                    {new Date(event.created_at).toLocaleString()}
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-xs">{actorLabel(event)}</TableCell>
                  <TableCell className="font-mono text-xs">{event.action}</TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    <p>{event.target_type}</p>
                    {event.target_id && <p className="font-mono">{event.target_id.slice(0, 8)}</p>}
                  </TableCell>
                  <TableCell>{resultBadge(event.result)}</TableCell>
                  <TableCell>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={`View details for ${event.action}`}
                      onClick={() => setSelected(event)}
                    >
                      <EyeIcon />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}

      {pagination && (
        <div className="flex items-center justify-between gap-3">
          <p className="text-sm text-muted-foreground">
            {pagination.total} {pagination.total === 1 ? "event" : "events"}
          </p>
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label="Previous page"
              disabled={pagination.page <= 1}
              onClick={() => setPage((value) => Math.max(1, value - 1))}
            >
              <ArrowLeftIcon />
            </Button>
            <span className="min-w-20 text-center text-sm text-muted-foreground">
              {pagination.page} / {Math.max(1, pagination.total_pages)}
            </span>
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label="Next page"
              disabled={pagination.page >= pagination.total_pages}
              onClick={() => setPage((value) => value + 1)}
            >
              <ArrowRightIcon />
            </Button>
          </div>
        </div>
      )}

      <Sheet open={selected !== null} onOpenChange={(open) => !open && setSelected(null)}>
        <SheetContent className="sm:max-w-xl">
          <SheetHeader>
            <SheetTitle>Audit event details</SheetTitle>
            <SheetDescription>{selected?.action}</SheetDescription>
          </SheetHeader>
          {selected && <EventDetails event={selected} />}
        </SheetContent>
      </Sheet>
    </div>
  );
}
