import { ExternalLinkIcon } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { apiUrl } from "@/lib/api";
import { authApi, type AuthConfiguration } from "./auth.api";

let configurationPromise: Promise<AuthConfiguration> | null = null;

export function useAuthConfiguration() {
  const [configuration, setConfiguration] = useState<AuthConfiguration | null>(null);
  useEffect(() => {
    let active = true;
    configurationPromise ??= authApi.configuration();
    configurationPromise
      .then((value) => {
        if (active) setConfiguration(value);
      })
      .catch(() => {
        configurationPromise = null;
      });
    return () => {
      active = false;
    };
  }, []);
  return configuration;
}

export function ProviderButtons({
  returnTo,
  invitationToken,
}: {
  returnTo?: string;
  invitationToken?: string | null;
}) {
  const configuration = useAuthConfiguration();

  if (!configuration?.providers.length) return null;

  return (
    <div className="grid gap-3">
      <div className="relative flex items-center">
        <div className="h-px flex-1 bg-border" />
        <span className="px-3 text-xs text-muted-foreground">or</span>
        <div className="h-px flex-1 bg-border" />
      </div>
      {configuration.providers.map((provider) => (
        <Button
          key={provider.slug}
          type="button"
          variant="outline"
          className="w-full"
          onClick={() => {
            const query = new URLSearchParams();
            if (returnTo) query.set("return_to", returnTo);
            if (invitationToken) query.set("invitation_token", invitationToken);
            const suffix = query.size ? `?${query}` : "";
            window.location.assign(
              apiUrl(`/api/v1/auth/providers/${encodeURIComponent(provider.slug)}/start${suffix}`),
            );
          }}
        >
          <ExternalLinkIcon />
          Continue with {provider.name}
        </Button>
      ))}
    </div>
  );
}
