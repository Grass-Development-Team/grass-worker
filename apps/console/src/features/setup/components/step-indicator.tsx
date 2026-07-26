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
  const current = STEPS.find((step) => step.stage === stage);
  return (
    <div className="flex flex-col items-center gap-2">
      <div className="flex items-center justify-center gap-1.5">
        {STEPS.map((step, i) => {
          const isCompleted = i < currentIndex;
          const isCurrent = step.stage === stage;
          return (
            <div key={step.stage} className="flex items-center gap-1.5">
              <div
                className={`flex size-7 items-center justify-center rounded-full text-xs font-medium transition-colors ${
                  isCompleted
                    ? "bg-primary text-primary-foreground"
                    : isCurrent
                      ? "border-2 border-primary text-primary"
                      : "border border-border text-muted-foreground"
                }`}
                title={step.title}
              >
                {isCompleted ? <Check className="size-3.5" /> : i + 1}
              </div>
              {i < STEPS.length - 1 && (
                <div className={`h-px w-5 ${isCompleted ? "bg-primary" : "bg-border"}`} />
              )}
            </div>
          );
        })}
      </div>
      {current && (
        <p className="text-xs text-muted-foreground">
          Step {currentIndex + 1} of {STEPS.length} — {current.title}
        </p>
      )}
    </div>
  );
}
