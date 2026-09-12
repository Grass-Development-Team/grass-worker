import { ArrowLeftIcon, type LucideIcon } from "lucide-react";
import { NavLink } from "react-router";

import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";

export function SettingsSidebarSections({
  title,
  back,
  sections,
}: {
  title: string;
  back: { to: string; label: string; tooltip: string };
  sections: { to: string; label: string; icon: LucideIcon; active: boolean }[];
}) {
  return (
    <>
      <SidebarGroup>
        <SidebarGroupContent>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton asChild tooltip={back.tooltip}>
                <NavLink to={back.to}>
                  <ArrowLeftIcon />
                  <span>{back.label}</span>
                </NavLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
      <SidebarGroup>
        <SidebarGroupLabel>{title}</SidebarGroupLabel>
        <SidebarGroupContent>
          <SidebarMenu>
            {sections.map((section) => (
              <SidebarMenuItem key={section.to}>
                <SidebarMenuButton asChild tooltip={section.label} isActive={section.active}>
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
