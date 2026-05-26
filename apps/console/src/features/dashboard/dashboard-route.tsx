import { ActivityIcon, CpuIcon, RocketIcon, ServerIcon } from "lucide-react";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

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
    <>
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
            <ActivityIcon className="size-5" /> Next milestone preview
          </CardTitle>
          <CardDescription>
            Database, migration, seed, and setup flow will attach real system state to this shell.
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
    </>
  );
}
