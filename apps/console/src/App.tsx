import { useQuery } from "@tanstack/react-query";
import { AlertCircleIcon, RefreshCwIcon } from "lucide-react";
import { useEffect } from "react";
import { useNavigate, useLocation } from "react-router";
import { Button } from "@/components/ui/button";
import { apiUrl } from "@/lib/api";
import { Router } from "./router";
import { AuthProvider } from "@/features/auth/auth-context";

interface HealthResponse {
  status: string;
  service: string;
  version: string;
  setup?: boolean;
}

async function fetchHealth(): Promise<HealthResponse> {
  const res = await fetch(apiUrl("/health"), { credentials: "include" });
  if (!res.ok) throw new Error("health check failed");
  return res.json();
}

export function App() {
  const navigate = useNavigate();
  const location = useLocation();

  const {
    data: health,
    isError,
    refetch,
  } = useQuery({
    queryKey: ["health"],
    queryFn: fetchHealth,
    retry: 3,
    retryDelay: 2000,
    refetchInterval: 3000,
  });

  const isSetupMode = health?.setup === true;

  useEffect(() => {
    if (!health) return;
    if (isSetupMode && location.pathname !== "/setup") {
      navigate("/setup", { replace: true });
    } else if (!isSetupMode && location.pathname === "/setup") {
      navigate("/login", { replace: true });
    }
  }, [health, isSetupMode, location.pathname, navigate]);

  if (isError) {
    return (
      <main className="flex min-h-svh items-center justify-center p-6">
        <div role="alert" className="flex max-w-sm flex-col items-center gap-4 text-center">
          <AlertCircleIcon className="size-8 text-destructive" />
          <div>
            <h1 className="font-semibold">Control API unavailable</h1>
            <p className="text-sm text-muted-foreground">
              The Console could not load the service health state.
            </p>
          </div>
          <Button variant="outline" onClick={() => refetch()}>
            <RefreshCwIcon data-icon="inline-start" />
            Retry
          </Button>
        </div>
      </main>
    );
  }

  if (!health) return null;

  if (
    (isSetupMode && location.pathname !== "/setup") ||
    (!isSetupMode && location.pathname === "/setup")
  ) {
    return null;
  }

  if (isSetupMode) {
    return <Router />;
  }

  return (
    <AuthProvider>
      <Router />
    </AuthProvider>
  );
}
