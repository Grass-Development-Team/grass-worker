import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRoundIcon, MoreHorizontalIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useEffect, useState } from "react";

import { SettingsCard } from "@/components/settings-card";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  adminApi,
  type AdminIdentityProvider,
  type AdminMfaPolicy,
  type AdminPasswordPolicy,
} from "../admin.api";

export function AuthenticationPanel() {
  const settings = useQuery({ queryKey: ["admin", "settings"], queryFn: adminApi.getSettings });
  const providers = useQuery({
    queryKey: ["admin", "identity-providers"],
    queryFn: adminApi.listIdentityProviders,
  });
  if (settings.isLoading || providers.isLoading) {
    return <Skeleton className="h-72 w-full" aria-busy="true" />;
  }
  if (settings.isError || providers.isError || !settings.data || !providers.data) return null;

  return (
    <div className="flex flex-col gap-6">
      <AuthenticationPolicyForm
        initialPassword={settings.data.authentication.password_policy}
        initialRegistrationVerification={
          settings.data.authentication.registration_email_verification
        }
        initialMfa={settings.data.authentication.mfa_policy}
        mailEnabled={settings.data.mail.mode !== "none"}
      />
      <IdentityProviders providers={providers.data.providers} />
    </div>
  );
}

function AuthenticationPolicyForm({
  initialPassword,
  initialRegistrationVerification,
  initialMfa,
  mailEnabled,
}: {
  initialPassword: AdminPasswordPolicy;
  initialRegistrationVerification: boolean;
  initialMfa: AdminMfaPolicy;
  mailEnabled: boolean;
}) {
  const queryClient = useQueryClient();
  const [password, setPassword] = useState(initialPassword);
  const [registrationVerification, setRegistrationVerification] = useState(
    initialRegistrationVerification,
  );
  const [mfa, setMfa] = useState(initialMfa);
  const [addingMethod, setAddingMethod] = useState(false);
  const mutation = useMutation({
    mutationFn: () =>
      adminApi.updateSettings({
        password_policy: password,
        registration_email_verification: registrationVerification,
        mfa_policy: mfa,
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["admin", "settings"] }),
  });

  const setPasswordValue = <K extends keyof AdminPasswordPolicy>(
    key: K,
    value: AdminPasswordPolicy[K],
  ) => setPassword((current) => ({ ...current, [key]: value }));
  const removeMethod = (factor: "totp" | "email") =>
    setMfa((current) => {
      const allowedFactors = current.allowed_factors.filter((item) => item !== factor);
      return {
        ...current,
        allowed_factors: allowedFactors,
        enforcement: allowedFactors.length === 0 ? "none" : current.enforcement,
        required_factors: current.required_factors.filter((item) => item !== factor),
        minimum_factors: Math.min(current.minimum_factors, allowedFactors.length),
      };
    });
  const addMethod = (factor: "totp" | "email") =>
    setMfa((current) => ({
      ...current,
      allowed_factors: [...new Set([...current.allowed_factors, factor])],
    }));
  const toggleRequired = (factor: "totp" | "email", checked: boolean) =>
    setMfa((current) => ({
      ...current,
      required_factors: checked
        ? [...new Set([...current.required_factors, factor])]
        : current.required_factors.filter((item) => item !== factor),
    }));

  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        mutation.mutate();
      }}
    >
      <SettingsCard
        title="Authentication policy"
        description="Password, registration verification, and MFA enforcement for the platform."
        action={
          <Button type="submit" size="sm" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving..." : "Save"}
          </Button>
        }
      >
        <FieldGroup>
          <div className="grid gap-4 sm:grid-cols-3">
            <Field>
              <FieldLabel htmlFor="password-min-length">Minimum length</FieldLabel>
              <Input
                id="password-min-length"
                type="number"
                min={8}
                max={128}
                value={password.min_length}
                onChange={(event) => setPasswordValue("min_length", Number(event.target.value))}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="password-max-length">Maximum length</FieldLabel>
              <Input
                id="password-max-length"
                type="number"
                min={password.min_length}
                max={1024}
                value={password.max_length}
                onChange={(event) => setPasswordValue("max_length", Number(event.target.value))}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="password-history">Password history</FieldLabel>
              <Input
                id="password-history"
                type="number"
                min={0}
                max={20}
                value={password.history_count}
                onChange={(event) => setPasswordValue("history_count", Number(event.target.value))}
              />
            </Field>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            {(
              [
                ["require_lowercase", "Require lowercase"],
                ["require_uppercase", "Require uppercase"],
                ["require_number", "Require number"],
                ["require_symbol", "Require symbol"],
              ] as const
            ).map(([key, label]) => (
              <Field key={key} orientation="horizontal">
                <FieldContent>
                  <FieldLabel htmlFor={key}>{label}</FieldLabel>
                </FieldContent>
                <Switch
                  id={key}
                  checked={password[key]}
                  onCheckedChange={(checked) => setPasswordValue(key, checked)}
                />
              </Field>
            ))}
          </div>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel htmlFor="registration-verification">Verify registration email</FieldLabel>
              <FieldDescription>
                New password accounts remain signed out until verified.
              </FieldDescription>
            </FieldContent>
            <Switch
              id="registration-verification"
              checked={registrationVerification}
              disabled={!mailEnabled}
              onCheckedChange={setRegistrationVerification}
            />
          </Field>
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div>
              <h2 className="text-sm font-medium">MFA methods</h2>
              <p className="text-xs text-muted-foreground">
                Add each method once, then configure its platform requirement.
              </p>
            </div>
            <Button type="button" size="sm" variant="outline" onClick={() => setAddingMethod(true)}>
              <PlusIcon /> Add method
            </Button>
          </div>
          <div className="overflow-x-auto rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Method</TableHead>
                  <TableHead>Availability</TableHead>
                  <TableHead>Platform requirement</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {mfa.allowed_factors.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={4}
                      className="h-20 text-center text-sm text-muted-foreground"
                    >
                      No MFA methods configured.
                    </TableCell>
                  </TableRow>
                ) : (
                  mfa.allowed_factors.map((factor) => {
                    const isEmail = factor === "email";
                    return (
                      <TableRow key={factor}>
                        <TableCell>
                          <p className="font-medium">
                            {isEmail ? "Email code" : "Authenticator app"}
                          </p>
                          <p className="text-xs text-muted-foreground">
                            {isEmail
                              ? "Delivered to the verified account email."
                              : "TOTP-compatible authenticator."}
                          </p>
                        </TableCell>
                        <TableCell>
                          <Badge variant={isEmail && !mailEnabled ? "destructive" : "success"}>
                            {isEmail && !mailEnabled ? "Mail unavailable" : "Available"}
                          </Badge>
                        </TableCell>
                        <TableCell>
                          <label className="flex items-center gap-2 text-sm">
                            <Checkbox
                              checked={mfa.required_factors.includes(factor)}
                              onCheckedChange={(checked) =>
                                toggleRequired(factor, checked === true)
                              }
                            />
                            Required for the selected scope
                          </label>
                        </TableCell>
                        <TableCell className="text-right">
                          <Button
                            type="button"
                            size="icon-sm"
                            variant="ghost"
                            aria-label={`Remove ${isEmail ? "email" : "authenticator app"} method`}
                            onClick={() => removeMethod(factor)}
                          >
                            <Trash2Icon />
                          </Button>
                        </TableCell>
                      </TableRow>
                    );
                  })
                )}
              </TableBody>
            </Table>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="mfa-enforcement">Enforcement scope</FieldLabel>
              <Select
                value={mfa.enforcement}
                onValueChange={(value) =>
                  setMfa((current) => ({
                    ...current,
                    enforcement: value as AdminMfaPolicy["enforcement"],
                    minimum_factors:
                      value !== "none" &&
                      current.minimum_factors === 0 &&
                      current.required_factors.length === 0
                        ? Math.min(1, current.allowed_factors.length)
                        : current.minimum_factors,
                  }))
                }
              >
                <SelectTrigger id="mfa-enforcement">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">Optional</SelectItem>
                  <SelectItem value="platform_admins">Platform admins</SelectItem>
                  <SelectItem value="all_users">All users</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel htmlFor="mfa-minimum-factors">Minimum enrolled methods</FieldLabel>
              <Input
                id="mfa-minimum-factors"
                type="number"
                min={0}
                max={mfa.allowed_factors.length}
                value={mfa.minimum_factors}
                onChange={(event) =>
                  setMfa((current) => ({
                    ...current,
                    minimum_factors: Math.max(
                      0,
                      Math.min(current.allowed_factors.length, Number(event.target.value)),
                    ),
                  }))
                }
              />
            </Field>
          </div>
        </FieldGroup>
      </SettingsCard>
      <AddMfaMethodDialog
        open={addingMethod}
        mailEnabled={mailEnabled}
        allowedFactors={mfa.allowed_factors}
        onClose={() => setAddingMethod(false)}
        onAdd={(factor) => {
          addMethod(factor);
          setAddingMethod(false);
        }}
      />
    </form>
  );
}

const mfaMethodOptions = [
  {
    value: "totp" as const,
    label: "Authenticator app",
    description: "Time-based one-time passwords.",
  },
  {
    value: "email" as const,
    label: "Email code",
    description: "One-time codes sent to verified email.",
  },
];

function AddMfaMethodDialog({
  open,
  mailEnabled,
  allowedFactors,
  onClose,
  onAdd,
}: {
  open: boolean;
  mailEnabled: boolean;
  allowedFactors: Array<"totp" | "email">;
  onClose: () => void;
  onAdd: (factor: "totp" | "email") => void;
}) {
  const available = mfaMethodOptions.filter(
    (method) => !allowedFactors.includes(method.value) && (method.value !== "email" || mailEnabled),
  );
  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Add MFA method</DialogTitle>
          <DialogDescription>
            Each method can be added once to the platform policy.
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-2">
          {available.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              All currently supported methods are configured.
            </p>
          ) : (
            available.map((method) => (
              <button
                key={method.value}
                type="button"
                className="flex items-start justify-between gap-4 rounded-md border p-3 text-left hover:bg-muted"
                onClick={() => onAdd(method.value)}
              >
                <span>
                  <span className="block text-sm font-medium">{method.label}</span>
                  <span className="block text-xs text-muted-foreground">{method.description}</span>
                </span>
                <PlusIcon className="mt-0.5 size-4 shrink-0" />
              </button>
            ))
          )}
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function IdentityProviders({ providers }: { providers: AdminIdentityProvider[] }) {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState<AdminIdentityProvider | "new" | null>(null);
  const remove = useMutation({
    mutationFn: adminApi.deleteIdentityProvider,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["admin", "identity-providers"] }),
  });
  const toggle = useMutation({
    mutationFn: (provider: AdminIdentityProvider) =>
      adminApi.updateIdentityProvider(provider.id, { enabled: !provider.enabled }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["admin", "identity-providers"] }),
  });

  return (
    <SettingsCard
      title="Identity providers"
      description="OIDC and GitHub providers available on login and signup."
      action={
        <Button type="button" size="sm" onClick={() => setEditing("new")}>
          <PlusIcon /> Add provider
        </Button>
      }
    >
      {providers.length === 0 ? (
        <p className="text-sm text-muted-foreground">No identity providers configured.</p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Provider</TableHead>
              <TableHead>Type</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {providers.map((provider) => (
              <TableRow key={provider.id}>
                <TableCell>
                  <p className="font-medium">{provider.name}</p>
                  <p className="text-xs text-muted-foreground">{provider.slug}</p>
                </TableCell>
                <TableCell className="uppercase">{provider.kind}</TableCell>
                <TableCell>
                  <Badge variant={provider.enabled ? "success" : "secondary"}>
                    {provider.enabled ? "Enabled" : "Disabled"}
                  </Badge>
                </TableCell>
                <TableCell className="text-right">
                  <div className="inline-flex items-center gap-1">
                    <Button
                      type="button"
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Edit ${provider.name}`}
                      onClick={() => setEditing(provider)}
                    >
                      <MoreHorizontalIcon />
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => toggle.mutate(provider)}
                    >
                      {provider.enabled ? "Disable" : "Enable"}
                    </Button>
                    <AlertDialog>
                      <AlertDialogTrigger asChild>
                        <Button
                          type="button"
                          size="icon-sm"
                          variant="ghost"
                          aria-label={`Delete ${provider.name}`}
                        >
                          <Trash2Icon />
                        </Button>
                      </AlertDialogTrigger>
                      <AlertDialogContent>
                        <AlertDialogHeader>
                          <AlertDialogTitle>Delete {provider.name}?</AlertDialogTitle>
                          <AlertDialogDescription>
                            Existing links for this provider will be removed.
                          </AlertDialogDescription>
                        </AlertDialogHeader>
                        <AlertDialogFooter>
                          <AlertDialogCancel>Cancel</AlertDialogCancel>
                          <AlertDialogAction onClick={() => remove.mutate(provider.id)}>
                            Delete
                          </AlertDialogAction>
                        </AlertDialogFooter>
                      </AlertDialogContent>
                    </AlertDialog>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      {editing && (
        <ProviderDialog
          provider={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            queryClient.invalidateQueries({ queryKey: ["admin", "identity-providers"] });
          }}
        />
      )}
    </SettingsCard>
  );
}

type ProviderTemplate = "google" | "apple" | "github" | "custom";

const templates: Record<ProviderTemplate, Partial<AdminIdentityProvider>> = {
  google: {
    name: "Google",
    slug: "google",
    issuer_url: "https://accounts.google.com",
    authorization_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo",
    jwks_url: "https://www.googleapis.com/oauth2/v3/certs",
    scopes: ["openid", "email", "profile"],
  },
  apple: {
    name: "Apple",
    slug: "apple",
    issuer_url: "https://appleid.apple.com",
    authorization_url: "https://appleid.apple.com/auth/authorize",
    token_url: "https://appleid.apple.com/auth/token",
    userinfo_url: null,
    jwks_url: "https://appleid.apple.com/auth/keys",
    scopes: ["openid", "email", "name"],
  },
  github: {
    name: "GitHub",
    slug: "github",
    issuer_url: null,
    authorization_url: "https://github.com/login/oauth/authorize",
    token_url: "https://github.com/login/oauth/access_token",
    userinfo_url: "https://api.github.com/user",
    jwks_url: null,
    scopes: ["read:user", "user:email"],
  },
  custom: {
    name: "",
    slug: "",
    issuer_url: "",
    authorization_url: "",
    token_url: "",
    userinfo_url: "",
    jwks_url: "",
    scopes: ["openid", "email", "profile"],
  },
};

function ProviderDialog({
  provider,
  onClose,
  onSaved,
}: {
  provider: AdminIdentityProvider | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [template, setTemplate] = useState<ProviderTemplate>("google");
  const [values, setValues] = useState(() => provider ?? templates.google);
  const [clientSecret, setClientSecret] = useState("");
  useEffect(() => {
    if (!provider) setValues(templates[template]);
  }, [provider, template]);
  const setValue = (key: keyof AdminIdentityProvider, value: unknown) =>
    setValues((current) => ({ ...current, [key]: value }));
  const mutation = useMutation({
    mutationFn: () => {
      const input = {
        name: values.name ?? "",
        client_id: values.client_id ?? "",
        issuer_url: values.issuer_url || undefined,
        authorization_url: values.authorization_url ?? "",
        token_url: values.token_url ?? "",
        userinfo_url: values.userinfo_url || undefined,
        jwks_url: values.jwks_url || undefined,
        scopes: values.scopes ?? [],
        ...(clientSecret ? { client_secret: clientSecret } : {}),
      };
      return provider
        ? adminApi.updateIdentityProvider(provider.id, input)
        : adminApi.createIdentityProvider({
            ...input,
            slug: values.slug ?? "",
            template,
            client_secret: clientSecret,
          });
    },
    onSuccess: onSaved,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{provider ? `Edit ${provider.name}` : "Add identity provider"}</DialogTitle>
          <DialogDescription>
            Client secrets are encrypted and never returned by the API.
          </DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            mutation.mutate();
          }}
        >
          {!provider && (
            <Field>
              <FieldLabel htmlFor="provider-template">Template</FieldLabel>
              <Select
                value={template}
                onValueChange={(value) => setTemplate(value as ProviderTemplate)}
              >
                <SelectTrigger id="provider-template">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="google">Google</SelectItem>
                  <SelectItem value="apple">Apple</SelectItem>
                  <SelectItem value="github">GitHub</SelectItem>
                  <SelectItem value="custom">Custom OIDC</SelectItem>
                </SelectContent>
              </Select>
            </Field>
          )}
          <div className="grid gap-4 sm:grid-cols-2">
            <ProviderInput
              label="Name"
              value={values.name}
              onChange={(value) => setValue("name", value)}
            />
            <ProviderInput
              label="Slug"
              value={values.slug}
              disabled={Boolean(provider)}
              onChange={(value) => setValue("slug", value)}
            />
          </div>
          <ProviderInput
            label="Client ID"
            value={values.client_id}
            onChange={(value) => setValue("client_id", value)}
          />
          <Field>
            <FieldLabel htmlFor="provider-client-secret">Client secret</FieldLabel>
            <Input
              id="provider-client-secret"
              type="password"
              autoComplete="new-password"
              value={clientSecret}
              placeholder={provider?.client_secret_configured ? "Configured" : ""}
              onChange={(event) => setClientSecret(event.target.value)}
              required={!provider}
            />
          </Field>
          {(provider || template === "custom") && (
            <>
              <ProviderInput
                label="Issuer URL"
                value={values.issuer_url}
                type="url"
                required={provider?.kind !== "github"}
                onChange={(value) => setValue("issuer_url", value)}
              />
              <ProviderInput
                label="Authorization URL"
                value={values.authorization_url}
                type="url"
                onChange={(value) => setValue("authorization_url", value)}
              />
              <ProviderInput
                label="Token URL"
                value={values.token_url}
                type="url"
                onChange={(value) => setValue("token_url", value)}
              />
              <ProviderInput
                label="Userinfo URL"
                value={values.userinfo_url}
                type="url"
                onChange={(value) => setValue("userinfo_url", value)}
              />
              <ProviderInput
                label="JWKS URL"
                value={values.jwks_url}
                type="url"
                required={provider?.kind !== "github"}
                onChange={(value) => setValue("jwks_url", value)}
              />
              <ProviderInput
                label="Scopes"
                value={(values.scopes ?? []).join(" ")}
                onChange={(value) => setValue("scopes", value.split(/\s+/).filter(Boolean))}
              />
            </>
          )}
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              <KeyRoundIcon /> {mutation.isPending ? "Saving..." : "Save provider"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ProviderInput({
  label,
  value,
  onChange,
  type,
  disabled,
  required = true,
}: {
  label: string;
  value: unknown;
  onChange: (value: string) => void;
  type?: string;
  disabled?: boolean;
  required?: boolean;
}) {
  const id = `provider-${label.toLowerCase().replaceAll(" ", "-")}`;
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        type={type}
        value={typeof value === "string" ? value : ""}
        onChange={(event) => onChange(event.target.value)}
        disabled={disabled}
        required={required && !label.startsWith("Userinfo")}
      />
    </Field>
  );
}
