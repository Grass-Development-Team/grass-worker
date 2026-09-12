import { ShieldCheckIcon, UserRoundIcon } from "lucide-react";
import { useLocation } from "react-router";

import { SettingsSidebarSections } from "@/components/settings-sidebar-sections";

const sections = [
  { to: "/account/profile", label: "Profile", icon: UserRoundIcon },
  { to: "/account/security", label: "Security", icon: ShieldCheckIcon },
];

export function AccountSidebarNav() {
  const location = useLocation();

  return (
    <SettingsSidebarSections
      title="Personal settings"
      back={{ to: "/", label: "Console", tooltip: "Back to the Console" }}
      sections={sections.map((section) => ({
        ...section,
        active: location.pathname === section.to,
      }))}
    />
  );
}
