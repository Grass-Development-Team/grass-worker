import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { Package } from "lucide-react";

import { setupApi } from "@/features/setup/setup.api";
import { StepIndicator } from "@/features/setup/components/step-indicator";
import { DatabaseStep } from "@/features/setup/components/database-step";
import { AdminStep } from "@/features/setup/components/admin-step";
import { SiteStep } from "@/features/setup/components/site-step";
import { NodeStep } from "@/features/setup/components/node-step";
import { StorageStep } from "@/features/setup/components/storage-step";
import { FinishStep } from "@/features/setup/components/finish-step";

export function SetupRoute() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [nodeToken, setNodeToken] = useState<string | null>(null);

  const { data: setupState, isLoading } = useQuery({
    queryKey: ["setup-state"],
    queryFn: setupApi.getSetupState,
    refetchInterval: 3000,
  });

  const currentStage = setupState?.stage ?? "database";

  useEffect(() => {
    if (currentStage === "complete") {
      navigate("/login", { replace: true });
    }
  }, [currentStage, navigate]);

  if (isLoading) {
    return (
      <main className="flex min-h-svh items-center justify-center p-6">
        <div className="text-muted-foreground">Loading setup state...</div>
      </main>
    );
  }

  return (
    <main className="flex min-h-svh flex-col items-center justify-center p-6">
      <div className="w-full max-w-md space-y-8">
        <div className="text-center space-y-2">
          <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-primary text-primary-foreground mx-auto">
            <Package className="size-6" />
          </div>
          <h1 className="text-2xl font-bold">Welcome to Grass Worker</h1>
          <p className="text-muted-foreground text-sm">
            Let&apos;s get your platform set up in a few steps.
          </p>
        </div>

        <StepIndicator stage={currentStage} />

        {currentStage === "database" && (
          <DatabaseStep
            onSuccess={() => queryClient.invalidateQueries({ queryKey: ["setup-state"] })}
          />
        )}
        {currentStage === "admin" && (
          <AdminStep
            onSuccess={() => queryClient.invalidateQueries({ queryKey: ["setup-state"] })}
          />
        )}
        {currentStage === "site" && (
          <SiteStep
            onSuccess={() => queryClient.invalidateQueries({ queryKey: ["setup-state"] })}
          />
        )}
        {currentStage === "node" && (
          <NodeStep
            onSuccess={(token) => {
              setNodeToken(token);
              queryClient.invalidateQueries({ queryKey: ["setup-state"] });
            }}
            token={nodeToken}
          />
        )}
        {currentStage === "storage" && (
          <StorageStep
            onSuccess={() => queryClient.invalidateQueries({ queryKey: ["setup-state"] })}
          />
        )}
        {currentStage === "finish" && (
          <FinishStep onSuccess={() => navigate("/login", { replace: true })} />
        )}
      </div>
    </main>
  );
}
