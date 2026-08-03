import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BellIcon, CheckCheckIcon } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Spinner } from "@/components/ui/spinner";
import { AnnouncementDialog, NotificationItems } from "./notification-items";
import { notificationsApi, type NotificationItem } from "./notifications.api";

export function NotificationBell() {
  const [open, setOpen] = useState(false);
  const [announcement, setAnnouncement] = useState<NotificationItem | null>(null);
  const queryClient = useQueryClient();
  const unreadQuery = useQuery({
    queryKey: ["notifications", "unread-count"],
    queryFn: notificationsApi.unreadCount,
    staleTime: 30_000,
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
  });
  const inboxQuery = useQuery({
    queryKey: ["notifications", "list", "inbox"],
    queryFn: () => notificationsApi.list(1, 8),
    enabled: open,
  });
  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: ["notifications", "list"] }),
      queryClient.invalidateQueries({ queryKey: ["notifications", "unread-count"] }),
    ]);
  const markRead = useMutation({
    mutationFn: notificationsApi.markRead,
    onSuccess: refresh,
  });
  const markAll = useMutation({
    mutationFn: notificationsApi.markAllRead,
    onSuccess: refresh,
  });
  const count = unreadQuery.data?.count ?? 0;
  const label = count > 0 ? `Notifications, ${count} unread` : "Notifications";

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="icon" className="relative" aria-label={label}>
            <BellIcon />
            {count > 0 && (
              <span className="absolute -right-1 -top-1 flex min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-semibold leading-4 text-destructive-foreground">
                {count > 99 ? "99+" : count}
              </span>
            )}
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="end"
          sideOffset={8}
          className="w-[min(26rem,calc(100vw-2rem))] overflow-hidden p-0"
        >
          <header className="flex h-14 items-center justify-between px-4">
            <div className="flex items-center gap-2">
              <h2 className="text-sm font-semibold">Inbox</h2>
              {count > 0 && <Badge variant="secondary">{count > 99 ? "99+" : count}</Badge>}
            </div>
          </header>

          <div className="max-h-[min(32rem,65vh)] overflow-y-auto border-y">
            {inboxQuery.isPending ? (
              <div className="flex min-h-32 items-center justify-center">
                <Spinner className="size-5" />
              </div>
            ) : inboxQuery.isError ? (
              <p role="alert" className="px-4 py-8 text-center text-sm text-destructive">
                {inboxQuery.error.message}
              </p>
            ) : inboxQuery.data.notifications.length === 0 ? (
              <p className="px-4 py-10 text-center text-sm text-muted-foreground">
                No notifications.
              </p>
            ) : (
              <NotificationItems
                notifications={inboxQuery.data.notifications}
                onAnnouncement={setAnnouncement}
                onOpen={(item) => {
                  if (!item.read_at) markRead.mutate(item.id);
                  setOpen(false);
                }}
              />
            )}
          </div>

          <footer className="grid grid-cols-2 gap-2 p-3">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => markAll.mutate()}
              disabled={markAll.isPending || count === 0}
            >
              <CheckCheckIcon data-icon="inline-start" />
              Mark all as read
            </Button>
            <Button asChild variant="outline" size="sm">
              <Link
                to="/notifications"
                aria-label="View all notifications"
                onClick={() => setOpen(false)}
              >
                View all
              </Link>
            </Button>
          </footer>
        </PopoverContent>
      </Popover>
      <AnnouncementDialog
        announcement={announcement}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setAnnouncement(null);
        }}
      />
    </>
  );
}
