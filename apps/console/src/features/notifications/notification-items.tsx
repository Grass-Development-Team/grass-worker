import { CircleAlertIcon, CircleCheckIcon, InfoIcon, type LucideIcon } from "lucide-react";
import { Link } from "react-router";

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
}: {
  notifications: NotificationItem[];
  onOpen?: (item: NotificationItem) => void;
}) {
  return (
    <div className="divide-y">
      {notifications.map((item) => {
        const { icon: Icon, className } = notificationIcon(item.action);
        return (
          <article key={item.id} className={cn(item.read_at && "opacity-65")}>
            <Link
              to={item.target_url}
              className="grid min-h-24 grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-3 px-4 py-4 transition-colors hover:bg-accent/50 focus-visible:bg-accent/50 focus-visible:outline-none"
              onClick={() => onOpen?.(item)}
            >
              <span
                className={cn(
                  "mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-full",
                  className,
                )}
              >
                <Icon className="size-4" />
              </span>
              <span className="min-w-0">
                <h2 className="text-sm leading-5">
                  <span className="font-semibold">{item.title}</span>
                  {item.reason && <span>: {item.reason}</span>}
                </h2>
                <span className="mt-1 block truncate text-xs text-muted-foreground">
                  {item.project.name} / {item.project.slug}
                </span>
              </span>
              <span className="flex min-w-14 items-center justify-end gap-2 pt-0.5 text-xs text-muted-foreground">
                {!item.read_at && (
                  <span className="size-2 shrink-0 rounded-full bg-primary" aria-label="Unread" />
                )}
                <time dateTime={item.created_at}>{formatTimestamp(item.created_at)}</time>
              </span>
            </Link>
          </article>
        );
      })}
    </div>
  );
}
