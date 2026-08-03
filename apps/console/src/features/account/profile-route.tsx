import { useState } from "react";

import { SettingsCard } from "@/components/settings-card";
import { Button } from "@/components/ui/button";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { useAuth } from "@/features/auth/auth-context";

export function ProfileRoute() {
  const { user, updateProfile } = useAuth();
  const [displayName, setDisplayName] = useState(user?.display_name ?? "");
  const [pending, setPending] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setPending(true);
    setSaved(false);
    setError(null);
    try {
      await updateProfile(displayName.trim() || null);
      setSaved(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to update profile.");
    } finally {
      setPending(false);
    }
  };

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-6">
      <header>
        <h1 className="text-2xl font-semibold">Personal settings</h1>
        <p className="text-sm text-muted-foreground">
          Manage the profile shown across the Console.
        </p>
      </header>
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
          {error && (
            <p role="alert" className="mt-3 text-sm text-destructive">
              {error}
            </p>
          )}
        </SettingsCard>
      </form>
    </div>
  );
}
