import { ActivityIcon, CpuIcon, RocketIcon, ServerIcon } from "lucide-react";

import { ConsoleSidebar } from "@/components/console-sidebar";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";

const cards = [
  {
    title: "Control API",
    description: "Bootstrap config, tracing, and /health are ready.",
    icon: ServerIcon,
  },
  {
    title: "Node",
    description: "Empty lifecycle process with architecture-aligned config.",
    icon: CpuIcon,
  },
  {
    title: "Deployments Page",
    description: "Placeholder shell for the first-stage deployment workflow.",
    icon: RocketIcon,
  },
];

export function DashboardRoute() {
  return (
    <SidebarProvider>
      <ConsoleSidebar />
      <SidebarInset>
        <header className="flex h-16 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger />
          <Separator orientation="vertical" className="data-[orientation=vertical]:h-4" />
          <div className="flex flex-1 items-center justify-between gap-4">
            <div>
              <p className="text-sm text-muted-foreground">Milestone 0</p>
              <h1 className="text-lg font-semibold">Engineering skeleton</h1>
            </div>
            <Button asChild variant="outline">
              <a href="/login">Open login</a>
            </Button>
          </div>
        </header>
        <main className="flex flex-1 flex-col gap-6 p-6">
          <section className="grid gap-4 md:grid-cols-3">
            {cards.map(({ title, description, icon: Icon }) => (
              <Card key={title}>
                <CardHeader>
                  <div className="flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
                    <Icon className="size-5" />
                  </div>
                  <CardTitle>{title}</CardTitle>
                  <CardDescription>{description}</CardDescription>
                </CardHeader>
                <CardContent>
                  <div className="h-2 rounded-full bg-muted">
                    <div className="h-2 w-2/3 rounded-full bg-primary" />
                  </div>
                </CardContent>
              </Card>
            ))}
          </section>
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <ActivityIcon className="size-5" />
                Next milestone preview
              </CardTitle>
              <CardDescription>
                Database, migration, seed, and setup flow will attach real system state to this
                shell.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid gap-3 text-sm md:grid-cols-3">
                <div className="rounded-lg border p-4">Setup mode detection</div>
                <div className="rounded-lg border p-4">Initial admin creation</div>
                <div className="rounded-lg border p-4">Ready mode transition</div>
              </div>
            </CardContent>
          </Card>
        </main>
      </SidebarInset>
    </SidebarProvider>
  );
}
