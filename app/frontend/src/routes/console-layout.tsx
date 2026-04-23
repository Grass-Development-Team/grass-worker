import { Outlet, useOutletContext } from "react-router-dom";
import { ConsoleSidebar } from "@/components/console/console-sidebar";
import type { ProtectedOutletContext } from "./protected-route";

export function ConsoleLayout() {
  const context = useOutletContext<ProtectedOutletContext>();

  return (
    <div className="min-h-screen bg-muted/30 lg:flex">
      <ConsoleSidebar currentUser={context.currentUser} />
      <main className="min-w-0 flex-1 px-4 py-6 sm:px-6 lg:px-8 lg:py-8">
        <div className="mx-auto w-full max-w-6xl">
          <Outlet context={context} />
        </div>
      </main>
    </div>
  );
}
