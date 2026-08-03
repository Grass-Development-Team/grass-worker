import { ArrowLeftIcon, LogOutIcon, MoonIcon, SunIcon, UserRoundIcon } from "lucide-react";
import { useTheme } from "next-themes";
import { useState } from "react";
import { Link, Outlet, useNavigate } from "react-router";

import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAuth } from "@/features/auth/auth-context";
import { NotificationBell } from "@/features/notifications/notification-bell";

const initials = (value: string) => value.slice(0, 2).toUpperCase();

function ThemeToggle() {
  const { setTheme } = useTheme();

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label="Change theme">
          <SunIcon className="dark:hidden" />
          <MoonIcon className="hidden dark:block" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onClick={() => setTheme("light")}>
          <SunIcon /> Light
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => setTheme("dark")}>
          <MoonIcon /> Dark
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => setTheme("system")}>System</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function ProjectCreateLayout() {
  const { user, logout } = useAuth();
  const navigate = useNavigate();
  const [actionError, setActionError] = useState<string | null>(null);

  const signOut = async () => {
    setActionError(null);
    try {
      await logout();
      navigate("/login", { replace: true });
    } catch (cause) {
      setActionError(cause instanceof Error ? cause.message : "Unable to log out.");
    }
  };

  return (
    <div className="flex min-h-svh flex-col bg-background text-foreground">
      <header className="relative flex h-14 shrink-0 items-center border-b px-4 md:px-6">
        <Button variant="ghost" size="sm" asChild>
          <Link to="/projects">
            <ArrowLeftIcon data-icon="inline-start" />
            Back
          </Link>
        </Button>
        <span className="ml-2 text-sm font-medium sm:hidden">New Project</span>
        <div className="pointer-events-none absolute left-1/2 hidden -translate-x-1/2 text-sm font-medium sm:block">
          New Project
        </div>
        <div className="ml-auto flex items-center gap-1">
          <NotificationBell />
          <ThemeToggle />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" aria-label="Open account menu">
                <Avatar className="size-7">
                  <AvatarFallback className="text-[10px]">
                    {initials(user?.display_name || user?.email || "GW")}
                  </AvatarFallback>
                </Avatar>
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
              <DropdownMenuLabel className="font-normal">
                <p className="truncate text-sm font-medium">{user?.display_name ?? user?.email}</p>
                <p className="truncate text-xs text-muted-foreground">{user?.email}</p>
              </DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuItem asChild>
                <Link to="/account/profile">
                  <UserRoundIcon /> Personal settings
                </Link>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={signOut}>
                <LogOutIcon /> Log out
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </header>
      {actionError && (
        <p
          role="alert"
          className="border-b border-destructive/30 px-4 py-2 text-sm text-destructive"
        >
          {actionError}
        </p>
      )}
      <main className="flex min-h-0 flex-1 flex-col">
        <Outlet />
      </main>
    </div>
  );
}
