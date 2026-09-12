import {
  KeyRoundIcon,
  MegaphoneIcon,
  MailIcon,
  PaletteIcon,
  ShieldCheckIcon,
  SlidersHorizontalIcon,
  WorkflowIcon,
} from "lucide-react";
import { useLocation } from "react-router";

import { SettingsSidebarSections } from "@/components/settings-sidebar-sections";

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
    <SettingsSidebarSections
      title="Settings"
      back={{ to: "/admin", label: "Administration", tooltip: "Back to Administration" }}
      sections={sections.map((section) => ({
        ...section,
        active: location.pathname === section.to,
      }))}
    />
  );
}
