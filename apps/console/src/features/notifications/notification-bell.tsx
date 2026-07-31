import { useQuery } from "@tanstack/react-query";
import { BellIcon } from "lucide-react";
import { Link } from "react-router";

import { Button } from "@/components/ui/button";
import { notificationsApi } from "./notifications.api";

export function NotificationBell() {
  const query = useQuery({
    queryKey: ["notifications", "unread-count"],
    queryFn: notificationsApi.unreadCount,
    staleTime: 30_000,
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
  });
  const count = query.data?.count ?? 0;
  const label = count > 0 ? `Notifications, ${count} unread` : "Notifications";

  return (
    <Button asChild variant="ghost" size="icon" className="relative">
      <Link to="/notifications" aria-label={label}>
        <BellIcon />
        {count > 0 && (
          <span className="absolute -right-1 -top-1 flex min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-semibold leading-4 text-destructive-foreground">
            {count > 99 ? "99+" : count}
          </span>
        )}
      </Link>
    </Button>
  );
}
