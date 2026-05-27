import { useQuery } from "@tanstack/react-query";
import { useNavigate, useLocation } from "react-router";
import { Router } from "./router";
import { AuthProvider } from "@/features/auth/auth-context";

interface HealthResponse {
  status: string;
  service: string;
  version: string;
  setup?: boolean;
}

async function fetchHealth(): Promise<HealthResponse> {
  const res = await fetch("/health");
  if (!res.ok) throw new Error("health check failed");
  return res.json();
}

export function App() {
  const navigate = useNavigate();
  const location = useLocation();

  const { data: health } = useQuery({
    queryKey: ["health"],
    queryFn: fetchHealth,
    retry: true,
    retryDelay: 2000,
    refetchInterval: 3000,
  });

  if (!health) return null;

  const isSetupMode = health.setup === true;

  if (isSetupMode && location.pathname !== "/setup") {
    navigate("/setup", { replace: true });
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
