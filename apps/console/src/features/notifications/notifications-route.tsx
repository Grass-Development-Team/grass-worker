import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeftIcon, ArrowRightIcon, ArrowUpRightIcon, CheckCheckIcon } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { notificationsApi } from "./notifications.api";

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function errorMessage(cause: unknown) {
  return cause instanceof Error ? cause.message : "Unable to update notifications.";
}

export function NotificationsRoute() {
  const queryClient = useQueryClient();
  const [page, setPage] = useState(1);
  const [actionError, setActionError] = useState<string | null>(null);
  const query = useQuery({
    queryKey: ["notifications", "list", page],
    queryFn: () => notificationsApi.list(page),
  });
  const unreadQuery = useQuery({
    queryKey: ["notifications", "unread-count"],
    queryFn: notificationsApi.unreadCount,
  });
  const refreshUnread = () =>
    queryClient.invalidateQueries({ queryKey: ["notifications", "unread-count"] });
  const markRead = useMutation({
    mutationFn: notificationsApi.markRead,
    onSuccess: async () => {
      setActionError(null);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["notifications", "list"] }),
        refreshUnread(),
      ]);
    },
    onError: (cause) => setActionError(errorMessage(cause)),
  });
  const markAll = useMutation({
    mutationFn: notificationsApi.markAllRead,
    onSuccess: async () => {
      setActionError(null);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["notifications", "list"] }),
        refreshUnread(),
      ]);
    },
    onError: (cause) => setActionError(errorMessage(cause)),
  });

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-wrap items-center justify-between gap-3 border-b pb-4">
        <div>
          <h1 className="text-xl font-semibold">Notifications</h1>
          {query.data && (
            <p className="mt-1 text-sm text-muted-foreground">
              {query.data.pagination.total} total
            </p>
          )}
        </div>
        <Button
          variant="outline"
          onClick={() => markAll.mutate()}
          disabled={markAll.isPending || (unreadQuery.data?.count ?? 0) === 0}
        >
          <CheckCheckIcon data-icon="inline-start" />
          {markAll.isPending ? "Marking..." : "Mark all as read"}
        </Button>
      </header>

      {actionError && (
        <p role="alert" className="border-l-2 border-destructive pl-3 text-sm text-destructive">
          {actionError}
        </p>
      )}

      {query.isPending ? (
        <div className="flex min-h-48 items-center justify-center">
          <Spinner className="size-5" />
        </div>
      ) : query.error ? (
        <div role="alert" className="border-l-2 border-destructive pl-3 text-sm text-destructive">
          {query.error.message}
        </div>
      ) : query.data.notifications.length === 0 ? (
        <div className="flex min-h-48 items-center justify-center border-y text-sm text-muted-foreground">
          No notifications.
        </div>
      ) : (
        <div className="divide-y border-y">
          {query.data.notifications.map((item) => (
            <article
              key={item.id}
              className={`grid gap-4 py-5 md:grid-cols-[minmax(0,1fr)_auto] ${
                item.read_at ? "opacity-75" : ""
              }`}
            >
              <div className="min-w-0 space-y-2">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="text-sm font-semibold">{item.title}</h2>
                  {!item.read_at && <Badge variant="secondary">Unread</Badge>}
                </div>
                <p className="text-sm text-muted-foreground">
                  <span className="font-medium text-foreground">{item.project.name}</span>
                  <span className="px-1.5 text-muted-foreground/60">/</span>
                  <span>{item.project.slug}</span>
                </p>
                <p className="text-xs text-muted-foreground">
                  {item.actor.label} · {formatTimestamp(item.created_at)}
                </p>
                {item.reason && (
                  <p className="border-l-2 pl-3 text-sm text-foreground/90">{item.reason}</p>
                )}
              </div>
              <div className="flex items-start justify-end">
                <Button asChild variant="ghost" size="icon">
                  <Link
                    to={item.target_url}
                    aria-label={`Open ${item.title}`}
                    onClick={() => {
                      if (!item.read_at) markRead.mutate(item.id);
                    }}
                  >
                    <ArrowUpRightIcon />
                  </Link>
                </Button>
              </div>
            </article>
          ))}
        </div>
      )}

      {query.data && query.data.pagination.total_pages > 1 && (
        <nav className="flex items-center justify-between" aria-label="Notification pages">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPage((current) => Math.max(1, current - 1))}
            disabled={page <= 1}
            aria-label="Previous page"
          >
            <ArrowLeftIcon />
          </Button>
          <span className="text-xs text-muted-foreground">
            {query.data.pagination.page} / {query.data.pagination.total_pages}
          </span>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPage((current) => current + 1)}
            disabled={page >= query.data.pagination.total_pages}
            aria-label="Next page"
          >
            <ArrowRightIcon />
          </Button>
        </nav>
      )}
    </div>
  );
}
