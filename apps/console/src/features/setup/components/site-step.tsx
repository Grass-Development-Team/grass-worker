import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { ArrowRight, Globe } from "lucide-react";

import { setupApi } from "@/features/setup/setup.api";
import { useBranding } from "@/features/branding/branding-context";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export function SiteStep({ onSuccess }: { onSuccess: () => void }) {
  const { siteName } = useBranding();
  const [name, setName] = useState(siteName);
  const [siteUrl, setSiteUrl] = useState(window.location.origin);
  const [publicBaseUrl, setPublicBaseUrl] = useState(window.location.origin);
  const mutation = useMutation({
    mutationFn: () => setupApi.configureSite(name, siteUrl, publicBaseUrl),
    onSuccess,
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Globe className="size-5" /> Configure Site
        </CardTitle>
        <CardDescription>Set the instance identity and public URLs.</CardDescription>
      </CardHeader>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          mutation.mutate();
        }}
      >
        <CardContent>
          <div className="grid gap-3">
            <Label htmlFor="site-name">Site Name</Label>
            <Input
              id="site-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="My deployment platform"
              required
            />
            <Label htmlFor="site-url">Site URL</Label>
            <Input
              id="site-url"
              type="url"
              value={siteUrl}
              onChange={(e) => setSiteUrl(e.target.value)}
              placeholder="https://console.example.com"
              required
            />
            <Label htmlFor="public-base-url">Public Base URL</Label>
            <Input
              id="public-base-url"
              type="url"
              value={publicBaseUrl}
              onChange={(e) => setPublicBaseUrl(e.target.value)}
              placeholder="https://sites.example.com"
              required
            />
          </div>
        </CardContent>
        <CardFooter>
          <Button type="submit" className="w-full" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving..." : "Save Site Configuration"}
            {!mutation.isPending && <ArrowRight className="ml-2 size-4" />}
          </Button>
        </CardFooter>
      </form>
    </Card>
  );
}
