import { AccountAvatar, AccountMenuContent } from "@/features/account/account-menu";
import { ArrowLeftIcon, MoonIcon, SunIcon } from "lucide-react";
import { useTheme } from "next-themes";
import { Link, Outlet } from "react-router";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { NotificationBell } from "@/features/notifications/notification-bell";

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
                <AccountAvatar className="size-7" />
              </Button>
            </DropdownMenuTrigger>
            <AccountMenuContent align="end" />
          </DropdownMenu>
        </div>
      </header>
      <main className="flex min-h-0 flex-1 flex-col">
        <Outlet />
      </main>
    </div>
  );
}
