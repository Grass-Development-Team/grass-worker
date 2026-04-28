import { FolderKanban, LogOut, Menu, Settings, Shield } from "lucide-react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { currentUserQueryKey, logout, type CurrentUser } from "@/api/auth";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";

type ConsoleSidebarProps = {
  currentUser: CurrentUser;
};

type NavigationItem = {
  href: string;
  label: string;
  icon: typeof FolderKanban;
  matches: (pathname: string) => boolean;
};

type NavigationGroup = {
  label?: string;
  items: NavigationItem[];
};

function matchesConsolePath(href: string, pathname: string) {
  return pathname === href || pathname.startsWith(`${href}/`);
}

const workspaceNavigationGroup: NavigationGroup = {
  label: "Workspace",
  items: [
    {
      href: "/projects",
      label: "Projects",
      icon: FolderKanban,
      matches: (pathname) => matchesConsolePath("/projects", pathname),
    },
    {
      href: "/settings",
      label: "User settings",
      icon: Settings,
      matches: (pathname) => matchesConsolePath("/settings", pathname),
    },
  ],
};

function navigationGroups(currentUser: CurrentUser): NavigationGroup[] {
  if (!currentUser.is_admin) {
    return [workspaceNavigationGroup];
  }

  return [
    workspaceNavigationGroup,
    {
      label: "Admin",
      items: [
        {
          href: "/admin/projects",
          label: "Project management",
          icon: Shield,
          matches: (pathname) => matchesConsolePath("/admin/projects", pathname),
        },
      ],
    },
  ];
}

function ConsoleNavigation({ currentUser }: ConsoleSidebarProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const groups = navigationGroups(currentUser);
  const signOutMutation = useMutation({
    mutationFn: logout,
    onSuccess: async () => {
      queryClient.setQueryData(currentUserQueryKey, null);
      await navigate(
        `/login?redirect=${encodeURIComponent(
          `${location.pathname}${location.search}`,
        )}`,
        { replace: true },
      );
    },
  });

  return (
    <div className="flex h-full flex-col gap-6">
      <div className="space-y-1">
        <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Console
        </p>
        <p className="text-lg font-semibold">Grass Worker</p>
      </div>

      <nav aria-label="Console navigation" className="space-y-6">
        {groups.map((group, groupIndex) => (
          <div className="space-y-2" key={group.label ?? `group-${groupIndex}`}>
            {group.label ? (
              <p className="px-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                {group.label}
              </p>
            ) : null}

            <div className="space-y-1">
              {group.items.map((item) => {
                const Icon = item.icon;
                const active = item.matches(location.pathname);

                return (
                  <Button
                    asChild
                    className={cn("w-full justify-start", active && "bg-muted")}
                    key={item.href}
                    variant="ghost"
                  >
                    <Link aria-current={active ? "page" : undefined} to={item.href}>
                      <Icon className="size-4" />
                      {item.label}
                    </Link>
                  </Button>
                );
              })}
            </div>

            {groupIndex < groups.length - 1 ? <Separator /> : null}
          </div>
        ))}
      </nav>

      <div className="mt-auto space-y-4">
        <Separator />

        <div className="space-y-2 text-sm">
          <p className="text-muted-foreground">Signed in as</p>
          <p className="break-all font-medium">{currentUser.email}</p>
          <p className="text-muted-foreground">
            {currentUser.is_admin ? "Administrator" : "User"}
          </p>
        </div>

        <Button
          className="w-full justify-start"
          disabled={signOutMutation.isPending}
          onClick={() => signOutMutation.mutate()}
          type="button"
          variant="outline"
        >
          <LogOut className="size-4" />
          {signOutMutation.isPending ? "Signing out..." : "Sign out"}
        </Button>
      </div>
    </div>
  );
}

export function ConsoleSidebar({ currentUser }: ConsoleSidebarProps) {
  return (
    <>
      <aside className="hidden min-h-screen w-72 shrink-0 border-r bg-background p-6 lg:block">
        <ConsoleNavigation currentUser={currentUser} />
      </aside>

      <div className="flex items-center justify-between border-b bg-background px-4 py-3 lg:hidden">
        <div>
          <p className="text-sm font-semibold">Grass Worker</p>
          <p className="text-xs text-muted-foreground">Console</p>
        </div>
        <Sheet>
          <SheetTrigger asChild>
            <Button
              aria-label="Open console navigation"
              size="icon"
              type="button"
              variant="outline"
            >
              <Menu className="size-4" />
            </Button>
          </SheetTrigger>
          <SheetContent side="left">
            <SheetHeader>
              <SheetTitle>Console</SheetTitle>
              <SheetDescription>Workspace navigation and session controls.</SheetDescription>
            </SheetHeader>
            <div className="mt-6 h-[calc(100%-5rem)]">
              <ConsoleNavigation currentUser={currentUser} />
            </div>
          </SheetContent>
        </Sheet>
      </div>
    </>
  );
}
