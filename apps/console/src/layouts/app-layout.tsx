import {
  ActivityIcon,
  FolderGitIcon,
  GaugeIcon,
  HomeIcon,
  LogOutIcon,
  MonitorIcon,
  MoonIcon,
  ScrollTextIcon,
  SettingsIcon,
  ShieldCheckIcon,
  SunIcon,
  UsersIcon,
} from "lucide-react";
import { useTheme } from "next-themes";
import { useEffect, useState } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router";

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
import { Separator } from "@/components/ui/separator";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
  useSidebar,
} from "@/components/ui/sidebar";
import { useAuth } from "@/features/auth/auth-context";
import { canViewTeamSettings } from "@/features/teams/team-permissions";
import { TeamSwitcher } from "@/features/teams/team-switcher";
import { useTeam } from "@/features/teams/team-context";

const primaryNavigation = [
  { title: "Overview", url: "/", icon: HomeIcon },
  { title: "Projects", url: "/projects", icon: FolderGitIcon },
  { title: "Usage", url: "/quota", icon: GaugeIcon },
];
const administrationNavigation = {
  title: "Administration",
  url: "/admin",
  icon: ShieldCheckIcon,
};
const settingsNavigation = [
  { title: "General", url: "/settings/team", icon: SettingsIcon },
  { title: "Members", url: "/settings/members", icon: UsersIcon },
  { title: "Audit", url: "/settings/audit", icon: ScrollTextIcon },
];

const initials = (value: string) => value.slice(0, 2).toUpperCase();

export function AppLayout() {
  return (
    <SidebarProvider>
      <AppLayoutContent />
    </SidebarProvider>
  );
}

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
        <DropdownMenuItem onClick={() => setTheme("system")}>
          <MonitorIcon /> System
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function AppLayoutContent() {
  const { user, logout } = useAuth();
  const { activeTeam, activeRole, error, refreshTeams } = useTeam();
  const location = useLocation();
  const navigate = useNavigate();
  const { isMobile, setOpenMobile } = useSidebar();
  const [actionError, setActionError] = useState<string | null>(null);
  const showSettings = activeRole ? canViewTeamSettings(activeRole) : false;
  const navigation =
    user?.platform_role === "admin"
      ? [...primaryNavigation, administrationNavigation]
      : primaryNavigation;

  useEffect(() => {
    if (isMobile) setOpenMobile(false);
  }, [isMobile, location.pathname, location.search, setOpenMobile]);

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
    <>
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton asChild>
                <NavLink to="/" aria-label="Grass Worker Console">
                  <ActivityIcon />
                  <span className="font-semibold">Grass Worker</span>
                </NavLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
          <TeamSwitcher />
        </SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupLabel>Workspace</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {navigation.map((item) => (
                  <SidebarMenuItem key={item.url}>
                    <SidebarMenuButton
                      asChild
                      isActive={
                        item.url === "/"
                          ? location.pathname === "/"
                          : location.pathname === item.url ||
                            location.pathname.startsWith(`${item.url}/`)
                      }
                    >
                      <NavLink to={item.url}>
                        <item.icon />
                        <span>{item.title}</span>
                      </NavLink>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
          {showSettings && (
            <SidebarGroup>
              <SidebarGroupLabel>Team settings</SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {settingsNavigation.map((item) => (
                    <SidebarMenuItem key={item.url}>
                      <SidebarMenuButton
                        asChild
                        isActive={
                          location.pathname === item.url ||
                          location.pathname.startsWith(`${item.url}/`)
                        }
                      >
                        <NavLink to={item.url}>
                          <item.icon />
                          <span>{item.title}</span>
                        </NavLink>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          )}
        </SidebarContent>
        <SidebarFooter>
          <SidebarMenu>
            <SidebarMenuItem>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <SidebarMenuButton tooltip={user?.display_name || user?.email || "Account"}>
                    <Avatar className="size-6">
                      <AvatarFallback className="text-[10px]">
                        {initials(user?.display_name || user?.email || "GW")}
                      </AvatarFallback>
                    </Avatar>
                    <span className="min-w-0 flex-1 truncate text-left">
                      {user?.display_name || user?.email}
                    </span>
                  </SidebarMenuButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent side="top" align="start" className="w-56">
                  <DropdownMenuLabel className="font-normal">
                    <p className="truncate text-sm font-medium">
                      {user?.display_name ?? user?.email}
                    </p>
                    <p className="truncate text-xs text-muted-foreground">{user?.email}</p>
                  </DropdownMenuLabel>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem onClick={signOut}>
                    <LogOutIcon /> Log out
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
      </Sidebar>
      <SidebarInset>
        <header className="flex h-14 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger />
          <Separator orientation="vertical" className="data-[orientation=vertical]:h-4" />
          <div className="min-w-0">
            <p className="truncate text-sm font-medium">{activeTeam?.name ?? "No team"}</p>
            <p className="text-xs capitalize text-muted-foreground">{activeRole ?? "Workspace"}</p>
          </div>
          <div className="ml-auto flex items-center gap-1">
            <ThemeToggle />
          </div>
        </header>
        <main className="flex flex-1 flex-col p-4 md:p-6">
          <div className="mx-auto flex w-full max-w-6xl flex-1 flex-col gap-6">
            {actionError && (
              <p
                role="alert"
                className="border-l-2 border-destructive pl-3 text-sm text-destructive"
              >
                {actionError}
              </p>
            )}
            {error ? (
              <div
                role="alert"
                className="flex min-h-64 flex-col items-center justify-center gap-4 text-center"
              >
                <div>
                  <h1 className="font-semibold">Unable to load this workspace</h1>
                  <p className="text-sm text-muted-foreground">{error.message}</p>
                </div>
                <Button variant="outline" onClick={() => refreshTeams()}>
                  Retry
                </Button>
              </div>
            ) : (
              <Outlet key={activeTeam?.id ?? "no-team"} />
            )}
          </div>
        </main>
      </SidebarInset>
    </>
  );
}
