import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { CheckCircle, Package } from "lucide-react";

import { setupApi } from "@/features/setup/setup.api";
import { StepIndicator } from "@/features/setup/components/step-indicator";
import { DatabaseStep } from "@/features/setup/components/database-step";
import { AdminStep } from "@/features/setup/components/admin-step";
import { SiteStep } from "@/features/setup/components/site-step";
import { NodeStep } from "@/features/setup/components/node-step";
import { StorageStep } from "@/features/setup/components/storage-step";
import { FinishStep } from "@/features/setup/components/finish-step";

export function SetupRoute() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [nodeToken, setNodeToken] = useState<string | null>(null);

  const {
    data: setupState,
    isLoading,
    isError,
  } = useQuery({
    queryKey: ["setup-state"],
    queryFn: setupApi.getSetupState,
    refetchInterval: 3000,
    retry: false,
  });

  const currentStage = isError ? "complete" : (setupState?.stage ?? "database");

  useEffect(() => {
    if (currentStage === "complete") {
      navigate("/login", { replace: true });
    }
  }, [currentStage, navigate]);

  if (isLoading) {
    return <div className="text-muted-foreground">Loading setup state...</div>;
  }

  const handleStepSuccess = () => {
    queryClient.invalidateQueries({ queryKey: ["setup-state"] });
  };

  const handleFinishSuccess = () => {
    queryClient.invalidateQueries({ queryKey: ["health"] });
    queryClient.invalidateQueries({ queryKey: ["setup-state"] });
  };

  return (
    <div className="w-full max-w-md space-y-8">
      <div className="text-center space-y-2">
        <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-xl bg-primary text-primary-foreground">
          <Package className="size-6" />
        </div>
        <h1 className="text-2xl font-bold">Welcome to Grass Worker</h1>
        <p className="text-sm text-muted-foreground">
          Let&apos;s get your platform set up in a few steps.
        </p>
      </div>

      <StepIndicator stage={currentStage} />

      {currentStage === "database" && <DatabaseStep onSuccess={handleStepSuccess} />}
      {currentStage === "admin" && <AdminStep onSuccess={handleStepSuccess} />}
      {currentStage === "site" && <SiteStep onSuccess={handleStepSuccess} />}
      {currentStage === "node" && (
        <NodeStep
          onSuccess={(token) => {
            setNodeToken(token);
            handleStepSuccess();
          }}
          token={nodeToken}
        />
      )}
      {currentStage === "storage" && <StorageStep onSuccess={handleStepSuccess} />}
      {currentStage === "finish" && <FinishStep onSuccess={handleFinishSuccess} />}
      {currentStage === "complete" && (
        <div className="flex flex-col items-center gap-4 rounded-lg border border-green-200 bg-green-50 p-8 text-center dark:border-green-800 dark:bg-green-950">
          <CheckCircle className="size-10 text-green-600 dark:text-green-400" />
          <div>
            <h2 className="text-lg font-semibold text-green-800 dark:text-green-200">
              Setup Complete
            </h2>
            <p className="text-sm text-green-700 dark:text-green-300">
              Your platform is ready. Redirecting to login...
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
