import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";

import { adminApi, type AdminSettings } from "../admin.api";

export function SettingsPanel() {
  const settingsQuery = useQuery({
    queryKey: ["admin", "settings"],
    queryFn: adminApi.getSettings,
  });

  if (settingsQuery.isLoading) {
    return <Skeleton className="h-64 w-full" aria-busy="true" />;
  }
  if (settingsQuery.isError || !settingsQuery.data) {
    return (
      <p role="alert" className="text-sm text-destructive">
        Unable to load platform settings.
      </p>
    );
  }
  return <SettingsForm initial={settingsQuery.data} />;
}

function SettingsForm({ initial }: { initial: AdminSettings }) {
  const queryClient = useQueryClient();
  const [siteName, setSiteName] = useState(initial.site.name ?? "");
  const [siteUrl, setSiteUrl] = useState(initial.site.url ?? "");
  const [publicBaseUrl, setPublicBaseUrl] = useState(initial.site.public_base_url ?? "");
  const [storageRoot, setStorageRoot] = useState(initial.storage.root);
  const [signupPolicy, setSignupPolicy] = useState(initial.signup.policy);
  const [reviewProduction, setReviewProduction] = useState(initial.review.production);
  const [reviewPreview, setReviewPreview] = useState(initial.review.preview);
  const [saved, setSaved] = useState(false);

  useEffect(
    () => setSaved(false),
    [siteName, siteUrl, publicBaseUrl, storageRoot, signupPolicy, reviewProduction, reviewPreview],
  );

  const mutation = useMutation({
    mutationFn: () =>
      adminApi.updateSettings({
        site_name: siteName,
        site_url: siteUrl,
        public_base_url: publicBaseUrl,
        storage_root: storageRoot,
        signup_policy: signupPolicy,
        review_production: reviewProduction,
        review_preview: reviewPreview,
      }),
    onSuccess: () => {
      setSaved(true);
      queryClient.invalidateQueries({ queryKey: ["admin", "settings"] });
    },
  });

  return (
    <form
      className="space-y-6"
      onSubmit={(event) => {
        event.preventDefault();
        mutation.mutate();
      }}
    >
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Site</CardTitle>
          <CardDescription>Identity and URLs of this Grass Worker installation.</CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="settings-site-name">Site name</FieldLabel>
              <Input
                id="settings-site-name"
                value={siteName}
                onChange={(event) => setSiteName(event.target.value)}
                required
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-site-url">Console URL</FieldLabel>
              <Input
                id="settings-site-url"
                type="url"
                value={siteUrl}
                onChange={(event) => setSiteUrl(event.target.value)}
                required
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-public-base-url">Public base URL</FieldLabel>
              <Input
                id="settings-public-base-url"
                type="url"
                value={publicBaseUrl}
                onChange={(event) => setPublicBaseUrl(event.target.value)}
                required
              />
            </Field>
          </FieldGroup>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Storage</CardTitle>
          <CardDescription>
            Where the Control API keeps artifacts and where Nodes derive their work directories.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Field>
            <FieldLabel htmlFor="settings-storage-root">Storage root</FieldLabel>
            <Input
              id="settings-storage-root"
              value={storageRoot}
              onChange={(event) => setStorageRoot(event.target.value)}
              required
            />
            <FieldDescription>
              Absolute path. Node work roots move to {"{root}"}/node; the generated local node
              config is updated automatically.
            </FieldDescription>
          </Field>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Policies</CardTitle>
          <CardDescription>Signup and release review defaults.</CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="settings-signup-policy">Signup policy</FieldLabel>
              <Select
                value={signupPolicy}
                onValueChange={(value) => setSignupPolicy(value as typeof signupPolicy)}
              >
                <SelectTrigger id="settings-signup-policy">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="open">Open — anyone can register</SelectItem>
                  <SelectItem value="invite_only">Invite only</SelectItem>
                  <SelectItem value="closed">Closed</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="settings-review-production">Production review</FieldLabel>
                <Select
                  value={reviewProduction}
                  onValueChange={(value) => setReviewProduction(value as typeof reviewProduction)}
                >
                  <SelectTrigger id="settings-review-production">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="manual">Manual — requires approval</SelectItem>
                    <SelectItem value="auto">Auto — activates when ready</SelectItem>
                  </SelectContent>
                </Select>
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-review-preview">Preview review</FieldLabel>
                <Select
                  value={reviewPreview}
                  onValueChange={(value) => setReviewPreview(value as typeof reviewPreview)}
                >
                  <SelectTrigger id="settings-review-preview">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="auto">Auto — activates when ready</SelectItem>
                    <SelectItem value="manual">Manual — requires approval</SelectItem>
                  </SelectContent>
                </Select>
              </Field>
            </div>
          </FieldGroup>
        </CardContent>
      </Card>

      <div className="flex items-center gap-3">
        <Button type="submit" disabled={mutation.isPending}>
          {mutation.isPending ? "Saving…" : "Save settings"}
        </Button>
        {saved && <p className="text-sm text-muted-foreground">Settings saved.</p>}
        {mutation.isError && (
          <p role="alert" className="text-sm text-destructive">
            {mutation.error instanceof Error ? mutation.error.message : "Unable to save settings."}
          </p>
        )}
      </div>
    </form>
  );
}
