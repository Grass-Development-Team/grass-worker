import {
  ArrowLeftIcon,
  KeyRoundIcon,
  MegaphoneIcon,
  MailIcon,
  PaletteIcon,
  ShieldCheckIcon,
  SlidersHorizontalIcon,
  WorkflowIcon,
} from "lucide-react";
import { NavLink, useLocation } from "react-router";

import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";

const sections = [
  { to: "/admin/settings/basic", label: "Basic", icon: PaletteIcon },
  { to: "/admin/settings/announcements", label: "Announcements", icon: MegaphoneIcon },
  { to: "/admin/settings/email", label: "Email", icon: MailIcon },
  { to: "/admin/settings/authentication", label: "Authentication", icon: KeyRoundIcon },
  { to: "/admin/settings/governance", label: "Governance", icon: ShieldCheckIcon },
  { to: "/admin/settings/infrastructure", label: "Infrastructure", icon: SlidersHorizontalIcon },
  { to: "/admin/settings/runtime", label: "Runtime", icon: WorkflowIcon },
];

export function SettingsSidebarNav() {
  const location = useLocation();

  return (
    <>
      <SidebarGroup>
        <SidebarGroupContent>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton asChild tooltip="Back to Administration">
                <NavLink to="/admin">
                  <ArrowLeftIcon />
                  <span>Administration</span>
                </NavLink>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
      <SidebarGroup>
        <SidebarGroupLabel>Settings</SidebarGroupLabel>
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
