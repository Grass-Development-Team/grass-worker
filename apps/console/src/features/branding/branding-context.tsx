import { createContext, useContext, useEffect, useMemo, useState } from "react";

import { APP_NAME, APP_VERSION } from "@/lib/constants";

export interface Branding {
  siteName: string;
  logoUrl?: string | null;
  version: string;
}

const defaultBranding: Branding = {
  siteName: APP_NAME,
  logoUrl: null,
  version: APP_VERSION,
};

type BrandingContextValue = Branding & {
  setPageTitle: (title: string | null) => void;
};

const BrandingContext = createContext<BrandingContextValue>({
  ...defaultBranding,
  setPageTitle: () => undefined,
});

export function BrandingProvider({
  branding,
  children,
}: {
  branding?: Branding;
  children: React.ReactNode;
}) {
  const value = { ...defaultBranding, ...branding };
  const [pageTitle, setPageTitle] = useState<string | null>(null);
  const context = useMemo(
    () => ({ ...value, setPageTitle }),
    [value.siteName, value.logoUrl, value.version],
  );

  useEffect(() => {
    document.title = pageTitle ? `${pageTitle} · ${value.siteName}` : value.siteName;
  }, [pageTitle, value.siteName]);

  useEffect(() => {
    const selector = 'link[data-branding-favicon="true"]';
    const existing = document.head.querySelector<HTMLLinkElement>(selector);

    if (!value.logoUrl) {
      existing?.remove();
      return;
    }

    const favicon = existing ?? document.createElement("link");
    favicon.rel = "icon";
    favicon.dataset.brandingFavicon = "true";
    favicon.href = value.logoUrl;
    if (!existing) document.head.appendChild(favicon);

    return () => favicon.remove();
  }, [value.logoUrl]);

  return <BrandingContext.Provider value={context}>{children}</BrandingContext.Provider>;
}

export function useBranding() {
  return useContext(BrandingContext);
}

export function usePageTitle(title: string | null) {
  const { setPageTitle } = useBranding();

  useEffect(() => {
    setPageTitle(title);
    return () => setPageTitle(null);
  }, [setPageTitle, title]);
}
