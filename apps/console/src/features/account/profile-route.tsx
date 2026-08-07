import { useState } from "react";

import { AvatarEditor } from "@/components/avatar-editor";
import { SettingsCard } from "@/components/settings-card";
import { Button } from "@/components/ui/button";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { useAuth } from "@/features/auth/auth-context";
import { apiUrl } from "@/lib/api";
import { showErrorToast } from "@/lib/toast";

export function ProfileRoute() {
  const { user, updateProfile, uploadAvatar, removeAvatar } = useAuth();
  const [displayName, setDisplayName] = useState(user?.display_name ?? "");
  const [pending, setPending] = useState(false);
  const [saved, setSaved] = useState(false);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setPending(true);
    setSaved(false);
    try {
      await updateProfile(displayName.trim() || null);
      setSaved(true);
    } catch (cause) {
      showErrorToast(cause);
    } finally {
      setPending(false);
    }
  };

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <header>
        <h1 className="text-2xl font-semibold">Personal settings</h1>
        <p className="text-sm text-muted-foreground">
          Manage the profile shown across the Console.
        </p>
      </header>
      <SettingsCard title="Avatar" description="Your image across the Console.">
        <AvatarEditor
          src={user?.avatar_url ? apiUrl(user.avatar_url) : null}
          fallback={(user?.display_name || user?.email || "GW").slice(0, 2).toUpperCase()}
          onUpload={uploadAvatar}
          onRemove={removeAvatar}
        />
      </SettingsCard>
      <form onSubmit={submit} onChange={() => setSaved(false)}>
        <SettingsCard
          title="Profile"
          description="Your account identity."
          action={
            <>
              {saved && !pending && <span className="text-xs text-muted-foreground">Saved.</span>}
              <Button type="submit" size="sm" disabled={pending}>
                {pending ? "Saving..." : "Save"}
              </Button>
            </>
          }
        >
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="profile-display-name">Display name</FieldLabel>
              <Input
                id="profile-display-name"
                value={displayName}
                maxLength={120}
                autoComplete="name"
                onChange={(event) => setDisplayName(event.target.value)}
              />
              <FieldDescription>Up to 120 characters.</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="profile-email">Email</FieldLabel>
              <Input id="profile-email" type="email" value={user?.email ?? ""} readOnly />
              <FieldDescription>The account email cannot be changed here.</FieldDescription>
            </Field>
          </FieldGroup>
        </SettingsCard>
      </form>
    </div>
  );
}
