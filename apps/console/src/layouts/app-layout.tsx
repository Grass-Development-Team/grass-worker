import { ActivityIcon, HomeIcon, LogOutIcon, SettingsIcon, UsersIcon } from "lucide-react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router";

import { Avatar, AvatarFallback } from "@/components/ui/avatar";
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
} from "@/components/ui/sidebar";
import { useAuth } from "@/features/auth/auth-context";
import { canViewTeamSettings } from "@/features/teams/team-permissions";
import { TeamSwitcher } from "@/features/teams/team-switcher";
import { useTeam } from "@/features/teams/team-context";

const primaryNavigation = [{ title: "Overview", url: "/", icon: HomeIcon }];
const settingsNavigation = [
  { title: "General", url: "/settings/team", icon: SettingsIcon },
  { title: "Members", url: "/settings/members", icon: UsersIcon },
];

const initials = (value: string) => value.slice(0, 2).toUpperCase();

export function AppLayout() {
  const { user, logout } = useAuth();
  const { activeTeam, activeRole } = useTeam();
  const location = useLocation();
  const navigate = useNavigate();
  const showSettings = activeRole ? canViewTeamSettings(activeRole) : false;

  const signOut = async () => {
    await logout();
    navigate("/login", { replace: true });
  };

  return (
    <SidebarProvider>
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton asChild>
                <NavLink to="/" aria-label="Grass Worker Console">
                  <ActivityIcon />
                  <span>Grass Worker</span>
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
                {primaryNavigation.map((item) => (
                  <SidebarMenuItem key={item.url}>
                    <SidebarMenuButton asChild isActive={location.pathname === item.url}>
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
                      <SidebarMenuButton asChild isActive={location.pathname === item.url}>
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
              <SidebarMenuButton onClick={signOut} tooltip="Log out">
                <Avatar className="size-6">
                  <AvatarFallback className="text-[10px]">
                    {initials(user?.display_name || user?.email || "GW")}
                  </AvatarFallback>
                </Avatar>
                <span className="min-w-0 flex-1 truncate text-left">
                  {user?.display_name || user?.email}
                </span>
                <LogOutIcon />
              </SidebarMenuButton>
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
        </header>
        <main className="flex flex-1 flex-col gap-6 p-4 md:p-6">
          <Outlet />
        </main>
      </SidebarInset>
    </SidebarProvider>
  );
}
