import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { SettingsCard } from "@/components/settings-card";
import { Skeleton } from "@/components/ui/skeleton";

import { adminApi, type AdminSettings } from "../admin.api";

type UpdateSettingsInput = Parameters<typeof adminApi.updateSettings>[0];

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

function useSettingsMutation() {
  const queryClient = useQueryClient();
  const [saved, setSaved] = useState(false);
  const mutation = useMutation({
    mutationFn: (input: UpdateSettingsInput) => adminApi.updateSettings(input),
    onSuccess: () => {
      setSaved(true);
      queryClient.invalidateQueries({ queryKey: ["admin", "settings"] });
    },
  });
  return { mutation, saved, setSaved };
}

function SaveAction({ pending, saved }: { pending: boolean; saved: boolean }) {
  return (
    <>
      {saved && !pending && <span className="text-xs text-muted-foreground">Saved.</span>}
      <Button type="submit" size="sm" disabled={pending}>
        {pending ? "Saving…" : "Save"}
      </Button>
    </>
  );
}

function MutationError({ mutation }: { mutation: { isError: boolean; error: unknown } }) {
  if (!mutation.isError) return null;
  return (
    <p role="alert" className="mt-3 text-sm text-destructive">
      {mutation.error instanceof Error ? mutation.error.message : "Unable to save settings."}
    </p>
  );
}

function SettingsForm({ initial }: { initial: AdminSettings }) {
  const [siteName, setSiteName] = useState(initial.site.name ?? "");
  const [siteUrl, setSiteUrl] = useState(initial.site.url ?? "");
  const [publicBaseUrl, setPublicBaseUrl] = useState(initial.site.public_base_url ?? "");
  const [storageRoot, setStorageRoot] = useState(initial.storage.root);
  const [signupPolicy, setSignupPolicy] = useState(initial.signup.policy);
  const [reviewProduction, setReviewProduction] = useState(initial.review.production);
  const [reviewPreview, setReviewPreview] = useState(initial.review.preview);

  const site = useSettingsMutation();
  const storage = useSettingsMutation();
  const policies = useSettingsMutation();

  return (
    <div className="space-y-6">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          site.mutation.mutate({
            site_name: siteName,
            site_url: siteUrl,
            public_base_url: publicBaseUrl,
          });
        }}
        onChange={() => site.setSaved(false)}
      >
        <SettingsCard
          title="Site"
          description="Identity and URLs of this Grass Worker installation."
          hint="The public base URL is used in generated links."
          action={<SaveAction pending={site.mutation.isPending} saved={site.saved} />}
        >
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
          <MutationError mutation={site.mutation} />
        </SettingsCard>
      </form>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          storage.mutation.mutate({ storage_root: storageRoot });
        }}
        onChange={() => storage.setSaved(false)}
      >
        <SettingsCard
          title="Storage"
          description="Where the Control API keeps artifacts and where Nodes derive their work directories."
          hint="Node work roots move with this path automatically."
          action={<SaveAction pending={storage.mutation.isPending} saved={storage.saved} />}
        >
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
          <MutationError mutation={storage.mutation} />
        </SettingsCard>
      </form>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          policies.mutation.mutate({
            signup_policy: signupPolicy,
            review_production: reviewProduction,
            review_preview: reviewPreview,
          });
        }}
      >
        <SettingsCard
          title="Policies"
          description="Signup and release review defaults."
          hint="Release review changes apply to new deployments."
          action={<SaveAction pending={policies.mutation.isPending} saved={policies.saved} />}
        >
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="settings-signup-policy">Signup policy</FieldLabel>
              <Select
                value={signupPolicy}
                onValueChange={(value) => {
                  setSignupPolicy(value as typeof signupPolicy);
                  policies.setSaved(false);
                }}
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
                  onValueChange={(value) => {
                    setReviewProduction(value as typeof reviewProduction);
                    policies.setSaved(false);
                  }}
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
                  onValueChange={(value) => {
                    setReviewPreview(value as typeof reviewPreview);
                    policies.setSaved(false);
                  }}
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
          <MutationError mutation={policies.mutation} />
        </SettingsCard>
      </form>
    </div>
  );
}
