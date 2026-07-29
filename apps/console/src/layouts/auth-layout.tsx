import { Outlet } from "react-router";
import { useBranding } from "@/features/branding/branding-context";

export function AuthLayout() {
  const { version } = useBranding();

  return (
    <main className="flex min-h-svh w-full flex-col p-6 md:p-10">
      <div className="flex flex-1 items-center justify-center">
        <Outlet />
      </div>
      <p className="shrink-0 pt-6 text-center text-[11px] text-muted-foreground">
        Powered by Grass Worker · v{version}
      </p>
    </main>
  );
}
