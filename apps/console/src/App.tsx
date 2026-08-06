import { useQuery } from "@tanstack/react-query";
import { RefreshCwIcon } from "lucide-react";
import { useEffect } from "react";
import { useNavigate, useLocation } from "react-router";
import { Button } from "@/components/ui/button";
import { apiUrl } from "@/lib/api";
import { Router } from "./router";
import { AuthProvider } from "@/features/auth/auth-context";
import { brandingApi } from "@/features/branding/branding.api";
import { BrandingProvider } from "@/features/branding/branding-context";

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
  const { data: siteConfig } = useQuery({
    queryKey: ["site-config"],
    queryFn: brandingApi.getSiteConfig,
    retry: false,
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
        <Button variant="outline" onClick={() => refetch()}>
          <RefreshCwIcon data-icon="inline-start" />
          Retry
        </Button>
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
    return (
      <BrandingProvider
        branding={
          siteConfig && {
            siteName: siteConfig.site_name,
            logoUrl: siteConfig.logo_url,
            version: siteConfig.version,
          }
        }
      >
        <Router />
      </BrandingProvider>
    );
  }

  return (
    <BrandingProvider
      branding={
        siteConfig && {
          siteName: siteConfig.site_name,
          logoUrl: siteConfig.logo_url,
          version: siteConfig.version,
        }
      }
    >
      <AuthProvider>
        <Router />
      </AuthProvider>
    </BrandingProvider>
  );
}
