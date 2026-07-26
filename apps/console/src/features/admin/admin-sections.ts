import {
  FolderGitIcon,
  GaugeIcon,
  GlobeIcon,
  InboxIcon,
  LayersIcon,
  ScrollTextIcon,
  ServerIcon,
  SettingsIcon,
  UserIcon,
  UsersIcon,
  type LucideIcon,
} from "lucide-react";

export interface AdminSection {
  to: string;
  label: string;
  icon: LucideIcon;
}

export const adminSections: AdminSection[] = [
  { to: "/admin/reviews", label: "Reviews", icon: InboxIcon },
  { to: "/admin/projects", label: "Projects", icon: FolderGitIcon },
  { to: "/admin/nodes", label: "Nodes", icon: ServerIcon },
  { to: "/admin/host-sources", label: "Host sources", icon: GlobeIcon },
  { to: "/admin/quota-plans", label: "Quota plans", icon: GaugeIcon },
  { to: "/admin/team-groups", label: "Team groups", icon: LayersIcon },
  { to: "/admin/users", label: "Users", icon: UserIcon },
  { to: "/admin/teams", label: "Teams", icon: UsersIcon },
  { to: "/admin/settings", label: "Settings", icon: SettingsIcon },
  { to: "/admin/audit", label: "Audit", icon: ScrollTextIcon },
];
