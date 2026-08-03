import {
  MegaphoneIcon,
  PaletteIcon,
  ShieldCheckIcon,
  SlidersHorizontalIcon,
  WorkflowIcon,
} from "lucide-react";
import { NavLink, Outlet } from "react-router";

const sections = [
  { to: "/admin/settings/basic", label: "Basic", icon: PaletteIcon },
  { to: "/admin/settings/announcements", label: "Announcements", icon: MegaphoneIcon },
  { to: "/admin/settings/governance", label: "Governance", icon: ShieldCheckIcon },
  { to: "/admin/settings/infrastructure", label: "Infrastructure", icon: SlidersHorizontalIcon },
  { to: "/admin/settings/runtime", label: "Runtime", icon: WorkflowIcon },
];

export function SettingsLayout() {
  return (
    <div className="flex flex-col gap-6">
      <nav className="flex flex-wrap gap-1 border-b pb-2" aria-label="Settings sections">
        {sections.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) =>
              `inline-flex items-center gap-2 border-b-2 px-3 py-2 text-sm transition-colors ${
                isActive
                  ? "border-foreground font-medium text-foreground"
                  : "border-transparent text-muted-foreground hover:text-foreground"
              }`
            }
          >
            <Icon className="size-4" />
            {label}
          </NavLink>
        ))}
      </nav>
      <Outlet />
    </div>
  );
}
