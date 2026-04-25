import { FolderKanban, LogOut, Menu, Shield } from "lucide-react";
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

const baseNavigationItems = [
  {
    href: "/projects",
    label: "Projects",
    icon: FolderKanban,
  },
];

function navigationItems(currentUser: CurrentUser) {
  if (!currentUser.is_admin) {
    return baseNavigationItems;
  }

  return [
    ...baseNavigationItems,
    {
      href: "/admin",
      label: "Admin",
      icon: Shield,
    },
  ];
}

function ConsoleNavigation({ currentUser }: ConsoleSidebarProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
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
    <div className="flex h-full flex-col gap-5">
      <div className="space-y-1">
        <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Console
        </p>
        <p className="text-lg font-semibold">Grass Worker</p>
      </div>

      <nav aria-label="Console navigation" className="space-y-1">
        {navigationItems(currentUser).map((item) => {
          const Icon = item.icon;
          const active = location.pathname === item.href ||
            location.pathname.startsWith(`${item.href}/`);

          return (
            <Button
              key={item.href}
              asChild
              className={cn("w-full justify-start", active && "bg-muted")}
              variant="ghost"
            >
              <Link to={item.href}>
                <Icon className="size-4" />
                {item.label}
              </Link>
            </Button>
          );
        })}
      </nav>

      <Separator />

      <div className="space-y-2 text-sm">
        <p className="text-muted-foreground">Signed in as</p>
        <p className="break-all font-medium">{currentUser.email}</p>
        <p className="text-muted-foreground">
          {currentUser.is_admin ? "Administrator" : "User"}
        </p>
      </div>

      <div className="mt-auto">
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
      <aside className="hidden min-h-screen w-72 shrink-0 border-r bg-card p-6 lg:block">
        <ConsoleNavigation currentUser={currentUser} />
      </aside>

      <div className="flex items-center justify-between border-b bg-card px-4 py-3 lg:hidden">
        <div>
          <p className="text-sm font-semibold">Grass Worker</p>
          <p className="text-xs text-muted-foreground">Console</p>
        </div>
        <Sheet>
          <SheetTrigger asChild>
            <Button aria-label="Open console navigation" size="icon" type="button" variant="outline">
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
