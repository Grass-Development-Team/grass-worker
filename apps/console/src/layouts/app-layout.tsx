import {
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
  UserRoundIcon,
  UsersIcon,
} from "lucide-react";
import { useTheme } from "next-themes";
import { useEffect, useState } from "react";
import { matchPath, NavLink, Outlet, useLocation, useNavigate } from "react-router";

import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { SiteLogo } from "@/components/site-logo";
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
import { Skeleton } from "@/components/ui/skeleton";
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
import { AdminSidebarNav } from "@/features/admin/components/admin-sidebar-nav";
import { SettingsSidebarNav } from "@/features/admin/components/settings-sidebar-nav";
import { adminSections } from "@/features/admin/admin-sections";
import { AccountSidebarNav } from "@/features/account/account-sidebar-nav";
import { useAuth } from "@/features/auth/auth-context";
import { useBranding, usePageTitle } from "@/features/branding/branding-context";
import {
  ProjectBreadcrumb,
  ProjectSidebarNav,
} from "@/features/projects/components/project-sidebar-nav";
import { canViewTeamAudit, canViewTeamSettings } from "@/features/teams/team-permissions";
import { TeamSwitcher } from "@/features/teams/team-switcher";
import { useTeam } from "@/features/teams/team-context";
import { NotificationBell } from "@/features/notifications/notification-bell";

const primaryNavigation = [
  { title: "Overview", url: "/", icon: HomeIcon },
  { title: "Projects", url: "/projects", icon: FolderGitIcon },
  { title: "Usage", url: "/quota", icon: GaugeIcon },
];
const settingsNavigation = [
  { title: "General", url: "/settings/team", icon: SettingsIcon },
  { title: "Members", url: "/settings/members", icon: UsersIcon },
  { title: "Audit", url: "/settings/audit", icon: ScrollTextIcon },
];

const initials = (value: string) => value.slice(0, 2).toUpperCase();

function pageTitle(pathname: string, inProject: boolean): string {
  if (pathname === "/") return "Overview";
  if (pathname === "/projects") return "Projects";
  if (inProject) {
    if (/\/deployments\/[^/]+$/.test(pathname)) return "Deployment";
    if (pathname.endsWith("/deployments")) return "Deployments";
    if (pathname.endsWith("/domains")) return "Domains";
    if (pathname.includes("/settings")) return "Project Settings";
    return "Overview";
  }
  if (pathname === "/quota") return "Usage";
  if (pathname === "/notifications") return "Notifications";
  if (pathname.startsWith("/account/profile")) return "Personal Settings";
  if (pathname.startsWith("/account/security")) return "Security";
  if (pathname.startsWith("/settings/team")) return "Team Settings";
  if (pathname.startsWith("/settings/members")) return "Members";
  if (pathname.startsWith("/settings/audit")) return "Audit";
  if (pathname === "/admin" || pathname.startsWith("/admin/")) {
    const section = adminSections.find(
      (candidate) => pathname === candidate.to || pathname.startsWith(`${candidate.to}/`),
    );
    return section?.label ?? "Administration";
  }
  return "";
}

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

function TeamSidebarNav({
  navigation,
  showSettings,
  showAudit,
}: {
  navigation: { title: string; url: string; icon: React.ComponentType }[];
  showSettings: boolean;
  showAudit: boolean;
}) {
  const location = useLocation();

  return (
    <>
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
              {settingsNavigation
                .filter((item) => item.title !== "Audit" || showAudit)
                .map((item) => (
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
    </>
  );
}

function AppLayoutContent() {
  const { user, logout } = useAuth();
  const { siteName, version } = useBranding();
  const { activeTeam, activeRole, error, isLoading, refreshTeams } = useTeam();
  const location = useLocation();
  const navigate = useNavigate();
  const { isMobile, setOpenMobile } = useSidebar();
  const [actionError, setActionError] = useState<string | null>(null);
  const showSettings = activeRole ? canViewTeamSettings(activeRole) : false;
  const showAudit = activeRole ? canViewTeamAudit(activeRole) : false;
  const navigation = primaryNavigation;

  const projectMatch = matchPath("/projects/:projectId/*", location.pathname);
  const projectId =
    projectMatch && projectMatch.params.projectId !== undefined
      ? projectMatch.params.projectId
      : null;
  const inAdmin = location.pathname === "/admin" || location.pathname.startsWith("/admin/");
  const inAdminSettings = location.pathname.startsWith("/admin/settings");
  const inAccountSettings = location.pathname.startsWith("/account/");
  const title = pageTitle(location.pathname, Boolean(projectId));
  usePageTitle(title || null);

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
                <NavLink to="/" aria-label={`${siteName} Console`}>
                  <SiteLogo className="size-4" />
                  <span className="font-semibold">{siteName}</span>
                </NavLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
          <TeamSwitcher />
        </SidebarHeader>
        <SidebarContent>
          {projectId ? (
            <ProjectSidebarNav projectId={projectId} />
          ) : inAdminSettings ? (
            <SettingsSidebarNav />
          ) : inAdmin ? (
            <AdminSidebarNav />
          ) : inAccountSettings ? (
            <AccountSidebarNav />
          ) : (
            <TeamSidebarNav
              navigation={navigation}
              showSettings={showSettings}
              showAudit={showAudit}
            />
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
                  <DropdownMenuItem asChild>
                    <NavLink to="/account/profile">
                      <UserRoundIcon /> Personal settings
                    </NavLink>
                  </DropdownMenuItem>
                  {user?.platform_role === "admin" && (
                    <DropdownMenuItem asChild>
                      <NavLink to="/admin">
                        <ShieldCheckIcon /> Administration
                      </NavLink>
                    </DropdownMenuItem>
                  )}
                  <DropdownMenuSeparator />
                  <DropdownMenuItem onClick={signOut}>
                    <LogOutIcon /> Log out
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </SidebarMenuItem>
          </SidebarMenu>
          <p className="px-2 text-[10px] text-muted-foreground group-data-[collapsible=icon]:hidden">
            Powered by Grass Worker · v{version}
          </p>
        </SidebarFooter>
      </Sidebar>
      <SidebarInset>
        <header className="relative flex h-14 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger />
          <Separator orientation="vertical" className="data-[orientation=vertical]:h-4" />
          <div className="flex min-w-0 items-center gap-2 text-sm">
            <NavLink
              to="/"
              className="truncate font-medium text-foreground/90 hover:text-foreground"
            >
              {activeTeam?.name ?? (isLoading ? "Loading workspace" : "No team")}
            </NavLink>
            {projectId && (
              <>
                <span className="text-muted-foreground/60">/</span>
                <ProjectBreadcrumb projectId={projectId} />
              </>
            )}
            {inAdmin && (
              <>
                <span className="text-muted-foreground/60">/</span>
                <span className="truncate text-sm font-medium">Administration</span>
              </>
            )}
          </div>
          {title && (
            <div className="pointer-events-none absolute left-1/2 hidden -translate-x-1/2 text-sm font-medium md:block">
              {title}
            </div>
          )}
          <div className="ml-auto flex items-center gap-1">
            <NotificationBell />
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
            ) : isLoading ? (
              <div className="space-y-4" aria-busy="true" role="status">
                <Skeleton className="h-8 w-56" />
                <Skeleton className="h-40 w-full" />
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
