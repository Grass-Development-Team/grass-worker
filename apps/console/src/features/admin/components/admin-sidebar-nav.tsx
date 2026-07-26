import { useQuery } from "@tanstack/react-query";
import { ArrowLeftIcon } from "lucide-react";
import { NavLink, useLocation } from "react-router";

import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";

import { adminApi } from "../admin.api";
import { adminSections } from "../admin-sections";

/** Sidebar navigation shown while inside Administration. */
export function AdminSidebarNav() {
  const location = useLocation();
  const reviews = useQuery({
    queryKey: ["admin", "reviews"],
    queryFn: adminApi.listReviews,
    refetchInterval: 60_000,
  });
  const pendingReviews = reviews.data?.total ?? 0;

  return (
    <>
      <SidebarGroup>
        <SidebarGroupContent>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton asChild tooltip="Back to the Console">
                <NavLink to="/">
                  <ArrowLeftIcon />
                  <span>Console</span>
                </NavLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
      <SidebarGroup>
        <SidebarGroupLabel>Administration</SidebarGroupLabel>
        <SidebarGroupContent>
          <SidebarMenu>
            {adminSections.map((section) => (
              <SidebarMenuItem key={section.to}>
                <SidebarMenuButton
                  asChild
                  tooltip={section.label}
                  isActive={
                    location.pathname === section.to ||
                    location.pathname.startsWith(`${section.to}/`)
                  }
                >
                  <NavLink to={section.to}>
                    <section.icon />
                    <span>{section.label}</span>
                  </NavLink>
                </SidebarMenuButton>
                {section.to === "/admin/reviews" && pendingReviews > 0 && (
                  <SidebarMenuBadge className="rounded-full bg-foreground px-1.5 text-background">
                    {pendingReviews}
                  </SidebarMenuBadge>
                )}
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
    </>
  );
}
