import { useQuery } from "@tanstack/react-query";
import { ArrowLeftIcon, GlobeIcon, HomeIcon, RocketIcon, SettingsIcon } from "lucide-react";
import { NavLink, useLocation } from "react-router";

import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from "@/components/ui/sidebar";

import { projectsApi } from "../projects.api";

function useProjectName(projectId: string): string | null {
  const projectQuery = useQuery({
    queryKey: ["project", projectId],
    queryFn: () => projectsApi.get(projectId),
    staleTime: 30_000,
  });
  return projectQuery.data?.project.name ?? null;
}

/** Header breadcrumb segment for the current project. */
export function ProjectBreadcrumb({ projectId }: { projectId: string }) {
  const name = useProjectName(projectId);
  return <span className="truncate text-sm font-medium">{name ?? "Project"}</span>;
}

/** Sidebar navigation shown while inside a project. */
export function ProjectSidebarNav({ projectId }: { projectId: string }) {
  const location = useLocation();
  const name = useProjectName(projectId);
  const base = `/projects/${projectId}`;
  const inSettings = location.pathname.startsWith(`${base}/settings`);

  const items = [
    { title: "Overview", url: base, icon: HomeIcon, exact: true },
    { title: "Deployments", url: `${base}/deployments`, icon: RocketIcon, exact: false },
    { title: "Domains", url: `${base}/domains`, icon: GlobeIcon, exact: false },
    { title: "Settings", url: `${base}/settings`, icon: SettingsIcon, exact: false },
  ];
  const settingsItems = [
    { title: "General", url: `${base}/settings`, exact: true },
    { title: "Build and Deployment", url: `${base}/settings/build-and-deployment`, exact: false },
  ];

  return (
    <>
      <SidebarGroup>
        <SidebarGroupContent>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton asChild tooltip="Back to Projects">
                <NavLink to="/projects">
                  <ArrowLeftIcon />
                  <span>Projects</span>
                </NavLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
      <SidebarGroup>
        <SidebarGroupLabel className="truncate">{name ?? "Project"}</SidebarGroupLabel>
        <SidebarGroupContent>
          <SidebarMenu>
            {items.map((item) => (
              <SidebarMenuItem key={item.url}>
                <SidebarMenuButton
                  asChild
                  tooltip={item.title}
                  isActive={
                    item.exact
                      ? location.pathname === item.url
                      : location.pathname === item.url ||
                        location.pathname.startsWith(`${item.url}/`)
                  }
                >
                  <NavLink to={item.url} end={item.exact}>
                    <item.icon />
                    <span>{item.title}</span>
                  </NavLink>
                </SidebarMenuButton>
                {item.title === "Settings" && inSettings && (
                  <SidebarMenuSub>
                    {settingsItems.map((settingsItem) => (
                      <SidebarMenuSubItem key={settingsItem.url}>
                        <SidebarMenuSubButton
                          asChild
                          isActive={
                            settingsItem.exact
                              ? location.pathname === settingsItem.url
                              : location.pathname.startsWith(settingsItem.url)
                          }
                        >
                          <NavLink to={settingsItem.url} end={settingsItem.exact}>
                            <span>{settingsItem.title}</span>
                          </NavLink>
                        </SidebarMenuSubButton>
                      </SidebarMenuSubItem>
                    ))}
                  </SidebarMenuSub>
                )}
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
    </>
  );
}
