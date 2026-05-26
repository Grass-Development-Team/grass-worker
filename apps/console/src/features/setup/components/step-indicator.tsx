import { Check, Database, Globe, Package, Server, User } from "lucide-react";

import type { SetupStage } from "@/features/setup/setup.api";

const STEPS = [
  { stage: "database", title: "Database", icon: Database },
  { stage: "admin", title: "Admin", icon: User },
  { stage: "site", title: "Site", icon: Globe },
  { stage: "node", title: "Node", icon: Server },
  { stage: "storage", title: "Storage", icon: Package },
  { stage: "finish", title: "Complete", icon: Check },
] as const satisfies { stage: SetupStage; title: string; icon: typeof Database }[];

const STAGE_ORDER: SetupStage[] = [
  "database",
  "admin",
  "site",
  "node",
  "storage",
  "finish",
  "complete",
];

export function StepIndicator({ stage }: { stage: SetupStage }) {
  const currentIndex = STAGE_ORDER.indexOf(stage);
  return (
    <div className="flex items-center justify-center gap-1">
      {STEPS.map((step, i) => {
        const isCompleted = i < currentIndex;
        const isCurrent = step.stage === stage;
        return (
          <div key={step.stage} className="flex items-center gap-1">
            <div
              className={`flex size-6 items-center justify-center rounded-full text-xs font-medium ${
                isCompleted
                  ? "bg-primary text-primary-foreground"
                  : isCurrent
                    ? "border-2 border-primary text-primary"
                    : "border-2 border-muted-foreground/30 text-muted-foreground"
              }`}
            >
              {isCompleted ? <Check className="size-3" /> : i + 1}
            </div>
            {i < STEPS.length - 1 && (
              <div
                className={`h-px w-4 ${isCompleted ? "bg-primary" : "bg-muted-foreground/30"}`}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}
