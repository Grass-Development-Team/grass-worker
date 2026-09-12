import { LogOutIcon, ShieldCheckIcon, UserRoundIcon } from "lucide-react";
import { NavLink, useNavigate } from "react-router";

import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import {
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu";
import { useAuth } from "@/features/auth/auth-context";
import { apiUrl } from "@/lib/api";
import { showErrorToast } from "@/lib/toast";

export function AccountAvatar({ className }: { className: string }) {
  const { user } = useAuth();
  return (
    <Avatar className={className}>
      {user?.avatar_url && (
        <AvatarImage src={apiUrl(user.avatar_url)} alt="" className="object-cover" />
      )}
      <AvatarFallback className="text-[10px]">
        {(user?.display_name || user?.email || "GW").slice(0, 2).toUpperCase()}
      </AvatarFallback>
    </Avatar>
  );
}

export function AccountMenuContent({
  showAdministration = false,
  ...position
}: {
  showAdministration?: boolean;
  align?: "start" | "center" | "end";
  side?: "top" | "right" | "bottom" | "left";
}) {
  const { user, logout } = useAuth();
  const navigate = useNavigate();
  const signOut = async () => {
    try {
      await logout();
      navigate("/login", { replace: true });
    } catch (cause) {
      showErrorToast(cause);
    }
  };
  return (
    <DropdownMenuContent {...position} className="w-56">
      <DropdownMenuGroup>
        <DropdownMenuLabel className="font-normal">
          <p className="truncate text-sm font-medium">{user?.display_name ?? user?.email}</p>
          <p className="truncate text-xs text-muted-foreground">{user?.email}</p>
        </DropdownMenuLabel>
      </DropdownMenuGroup>
      <DropdownMenuSeparator />
      <DropdownMenuGroup>
        <DropdownMenuItem asChild>
          <NavLink to="/account/profile">
            <UserRoundIcon /> Personal settings
          </NavLink>
        </DropdownMenuItem>
        {showAdministration && (
          <DropdownMenuItem asChild>
            <NavLink to="/admin">
              <ShieldCheckIcon /> Administration
            </NavLink>
          </DropdownMenuItem>
        )}
      </DropdownMenuGroup>
      <DropdownMenuSeparator />
      <DropdownMenuGroup>
        <DropdownMenuItem onClick={signOut}>
          <LogOutIcon /> Log out
        </DropdownMenuItem>
      </DropdownMenuGroup>
    </DropdownMenuContent>
  );
}
