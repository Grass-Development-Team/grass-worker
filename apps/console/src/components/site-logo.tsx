import { useEffect, useState } from "react";
import { ActivityIcon } from "lucide-react";

import { useBranding } from "@/features/branding/branding-context";

export function SiteLogo({ className }: { className?: string }) {
  const { logoUrl, siteName } = useBranding();
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  const showImage = Boolean(logoUrl && logoUrl !== failedUrl);

  useEffect(() => {
    setFailedUrl(null);
  }, [logoUrl]);

  if (showImage) {
    return (
      <img
        src={logoUrl ?? undefined}
        alt=""
        className={className}
        onError={() => setFailedUrl(logoUrl ?? null)}
      />
    );
  }

  return <ActivityIcon className={className} aria-label={`${siteName} logo`} />;
}
