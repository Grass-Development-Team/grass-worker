import { ArrowLeftIcon, UserRoundIcon } from "lucide-react";
import { NavLink, useLocation } from "react-router";

import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";

const sections = [{ to: "/account/profile", label: "Profile", icon: UserRoundIcon }];

export function AccountSidebarNav() {
  const location = useLocation();

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
        <SidebarGroupLabel>Personal settings</SidebarGroupLabel>
        <SidebarGroupContent>
          <SidebarMenu>
            {sections.map((section) => (
              <SidebarMenuItem key={section.to}>
                <SidebarMenuButton
                  asChild
                  tooltip={section.label}
                  isActive={location.pathname === section.to}
                >
                  <NavLink to={section.to}>
                    <section.icon />
                    <span>{section.label}</span>
                  </NavLink>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
    </>
  );
}
