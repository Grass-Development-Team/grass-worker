import {
  CircleAlertIcon,
  CircleCheckIcon,
  InfoIcon,
  MegaphoneIcon,
  type LucideIcon,
} from "lucide-react";
import { Link } from "react-router";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

import type { NotificationItem } from "./notifications.api";

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  }).format(new Date(value));
}

function notificationIcon(action: string): {
  icon: LucideIcon;
  className: string;
} {
  if (action === "site.announcement") {
    return { icon: MegaphoneIcon, className: "bg-primary/10 text-primary" };
  }
  if (
    action.endsWith(".approved") ||
    action.endsWith(".restored") ||
    action.endsWith(".unarchived") ||
    action.includes("republish")
  ) {
    return { icon: CircleCheckIcon, className: "bg-success/10 text-success" };
  }
  if (
    action.endsWith(".withdrawn") ||
    action.endsWith(".rejected") ||
    action.endsWith(".deleted") ||
    action.endsWith(".archived")
  ) {
    return { icon: CircleAlertIcon, className: "bg-warning/10 text-warning" };
  }
  return { icon: InfoIcon, className: "bg-muted text-muted-foreground" };
}

export function NotificationItems({
  notifications,
  onOpen,
  onAnnouncement,
}: {
  notifications: NotificationItem[];
  onOpen?: (item: NotificationItem) => void;
  onAnnouncement?: (item: NotificationItem) => void;
}) {
  return (
    <div className="divide-y">
      {notifications.map((item) => {
        const { icon: Icon, className } = notificationIcon(item.action);
        return (
          <article key={item.id} className={cn(item.read_at && "opacity-65")}>
            {item.action === "site.announcement" ? (
              <button
                type="button"
                className="grid min-h-24 w-full grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-3 px-4 py-4 text-left transition-colors hover:bg-accent/50 focus-visible:bg-accent/50 focus-visible:outline-none"
                onClick={() => {
                  onOpen?.(item);
                  onAnnouncement?.(item);
                }}
              >
                <NotificationContent item={item} icon={Icon} iconClassName={className} />
              </button>
            ) : (
              <Link
                to={item.target_url}
                className="grid min-h-24 grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-3 px-4 py-4 transition-colors hover:bg-accent/50 focus-visible:bg-accent/50 focus-visible:outline-none"
                onClick={() => onOpen?.(item)}
              >
                <NotificationContent item={item} icon={Icon} iconClassName={className} />
              </Link>
            )}
          </article>
        );
      })}
    </div>
  );
}

function NotificationContent({
  item,
  icon: Icon,
  iconClassName,
}: {
  item: NotificationItem;
  icon: LucideIcon;
  iconClassName: string;
}) {
  return (
    <>
      <span
        className={cn(
          "mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-full",
          iconClassName,
        )}
      >
        <Icon className="size-4" />
      </span>
      <span className="min-w-0">
        <h2 className="text-sm leading-5">
          <span className="font-semibold">{item.title}</span>
          {item.reason && <span>: {item.reason}</span>}
        </h2>
        {item.project ? (
          <span className="mt-1 block truncate text-xs text-muted-foreground">
            {item.project.name} / {item.project.slug}
          </span>
        ) : (
          <span className="mt-1 block text-xs text-muted-foreground">Website announcement</span>
        )}
      </span>
      <span className="flex min-w-14 items-center justify-end gap-2 pt-0.5 text-xs text-muted-foreground">
        {!item.read_at && (
          <span className="size-2 shrink-0 rounded-full bg-primary" aria-label="Unread" />
        )}
        <time dateTime={item.created_at}>{formatTimestamp(item.created_at)}</time>
      </span>
    </>
  );
}

export function AnnouncementDialog({
  announcement,
  onOpenChange,
}: {
  announcement: NotificationItem | null;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog open={Boolean(announcement)} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[min(42rem,calc(100vh-2rem))] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{announcement?.title ?? "Announcement"}</DialogTitle>
          <DialogDescription>
            {announcement?.created_at && (
              <time dateTime={announcement.created_at}>
                {new Intl.DateTimeFormat(undefined, {
                  dateStyle: "medium",
                  timeStyle: "short",
                }).format(new Date(announcement.created_at))}
              </time>
            )}
          </DialogDescription>
        </DialogHeader>
        <div className="whitespace-pre-wrap break-words text-sm leading-6">
          {announcement?.content}
        </div>
      </DialogContent>
    </Dialog>
  );
}
