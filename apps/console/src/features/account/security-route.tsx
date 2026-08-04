import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MailIcon, PlusIcon, SmartphoneIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { QRCodeSVG } from "qrcode.react";

import { SettingsCard } from "@/components/settings-card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { authApi, type MfaFactor, type TotpEnrollment } from "@/features/auth/auth.api";

export function SecurityRoute() {
  const queryClient = useQueryClient();
  const security = useQuery({ queryKey: ["account", "security"], queryFn: authApi.security });
  const [totp, setTotp] = useState<TotpEnrollment | null>(null);
  const [emailFactor, setEmailFactor] = useState<MfaFactor | null>(null);
  const [factorError, setFactorError] = useState<string | null>(null);

  const refresh = () => queryClient.invalidateQueries({ queryKey: ["account", "security"] });
  const startTotp = useMutation({
    mutationFn: authApi.accountTotpStart,
    onSuccess: (result) => {
      setTotp(result);
      setFactorError(null);
    },
    onError: (cause) =>
      setFactorError(cause instanceof Error ? cause.message : "Unable to start TOTP enrollment."),
  });
  const startEmail = useMutation({
    mutationFn: authApi.accountEmailStart,
    onSuccess: ({ factor }) => {
      setEmailFactor(factor);
      setFactorError(null);
    },
    onError: (cause) =>
      setFactorError(cause instanceof Error ? cause.message : "Unable to send the email code."),
  });
  const remove = useMutation({
    mutationFn: authApi.accountMfaDelete,
    onSuccess: refresh,
    onError: (cause) =>
      setFactorError(cause instanceof Error ? cause.message : "Unable to remove the factor."),
  });

  if (security.isLoading) return <Skeleton className="h-64 w-full" aria-busy="true" />;
  if (security.isError || !security.data) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {security.error instanceof Error
          ? security.error.message
          : "Unable to load security settings."}
      </p>
    );
  }

  const hasTotp = security.data.factors.some((factor) => factor.kind === "totp");
  const hasEmail = security.data.factors.some((factor) => factor.kind === "email");

  return (
    <div className="flex flex-col gap-6">
      <PasswordForm policy={security.data.password_policy} />
      <SettingsCard
        title="Multi-factor authentication"
        description="Factors used to verify future sign-ins."
      >
        <div className="grid gap-4">
          {security.data.factors.length === 0 ? (
            <p className="text-sm text-muted-foreground">No factors enrolled.</p>
          ) : (
            security.data.factors.map((factor) => (
              <div
                key={factor.id}
                className="flex min-h-12 items-center justify-between gap-4 border-b pb-3 last:border-0 last:pb-0"
              >
                <div className="flex min-w-0 items-center gap-3">
                  {factor.kind === "totp" ? (
                    <SmartphoneIcon className="size-4 shrink-0" />
                  ) : (
                    <MailIcon className="size-4 shrink-0" />
                  )}
                  <div>
                    <p className="text-sm font-medium">
                      {factor.kind === "totp" ? "Authenticator app" : "Email code"}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      Added {new Date(factor.created_at).toLocaleDateString()}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  {security.data.mfa_required && <Badge variant="secondary">Required</Badge>}
                  <Button
                    type="button"
                    size="icon-sm"
                    variant="ghost"
                    aria-label={`Remove ${factor.kind} factor`}
                    onClick={() => remove.mutate(factor.id)}
                    disabled={remove.isPending}
                  >
                    <Trash2Icon />
                  </Button>
                </div>
              </div>
            ))
          )}
          <div className="flex flex-wrap gap-2">
            {security.data.allowed_factors.includes("totp") && !hasTotp && (
              <Button type="button" variant="outline" onClick={() => startTotp.mutate()}>
                <PlusIcon /> Authenticator app
              </Button>
            )}
            {security.data.allowed_factors.includes("email") &&
              security.data.mail_available &&
              security.data.email_verified &&
              !hasEmail && (
                <Button type="button" variant="outline" onClick={() => startEmail.mutate()}>
                  <PlusIcon /> Email code
                </Button>
              )}
          </div>
          {factorError && (
            <p role="alert" className="text-sm text-destructive">
              {factorError}
            </p>
          )}
        </div>
      </SettingsCard>

      {totp && (
        <FactorConfirmationDialog
          title="Add authenticator app"
          description="Scan the QR code, then enter the current code."
          factor={totp.factor}
          onClose={() => setTotp(null)}
          onConfirmed={() => {
            setTotp(null);
            refresh();
          }}
        >
          <div className="grid justify-items-center gap-3">
            <div className="bg-white p-3">
              <QRCodeSVG value={totp.otpauth_uri} size={176} />
            </div>
            <code className="max-w-full break-all rounded bg-muted px-2 py-1 text-xs">
              {totp.secret}
            </code>
          </div>
        </FactorConfirmationDialog>
      )}
      {emailFactor && (
        <FactorConfirmationDialog
          title="Add email code"
          description="Enter the code sent to your verified email."
          factor={emailFactor}
          onClose={() => setEmailFactor(null)}
          onConfirmed={() => {
            setEmailFactor(null);
            refresh();
          }}
        />
      )}
    </div>
  );
}

function PasswordForm({
  policy,
}: {
  policy: Awaited<ReturnType<typeof authApi.security>>["password_policy"];
}) {
  const [currentPassword, setCurrentPassword] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const mutation = useMutation({
    mutationFn: () => authApi.changePassword(currentPassword, password),
    onSuccess: () => {
      setCurrentPassword("");
      setPassword("");
      setConfirm("");
      setSaved(true);
      setError(null);
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to change the password."),
  });

  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        setSaved(false);
        if (password !== confirm) {
          setError("Passwords do not match.");
          return;
        }
        mutation.mutate();
      }}
    >
      <SettingsCard
        title="Password"
        description="Change the password used for email sign-in."
        action={
          <Button type="submit" size="sm" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving..." : "Save"}
          </Button>
        }
      >
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="current-password">Current password</FieldLabel>
            <Input
              id="current-password"
              type="password"
              autoComplete="current-password"
              value={currentPassword}
              onChange={(event) => setCurrentPassword(event.target.value)}
              required
            />
          </Field>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="new-password">New password</FieldLabel>
              <Input
                id="new-password"
                type="password"
                autoComplete="new-password"
                minLength={policy.min_length}
                maxLength={policy.max_length}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                required
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="confirm-password">Confirm password</FieldLabel>
              <Input
                id="confirm-password"
                type="password"
                autoComplete="new-password"
                value={confirm}
                onChange={(event) => setConfirm(event.target.value)}
                required
              />
            </Field>
          </div>
          {saved && <p className="text-sm text-muted-foreground">Password updated.</p>}
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          )}
        </FieldGroup>
      </SettingsCard>
    </form>
  );
}

function FactorConfirmationDialog({
  title,
  description,
  factor,
  onClose,
  onConfirmed,
  children,
}: {
  title: string;
  description: string;
  factor: MfaFactor;
  onClose: () => void;
  onConfirmed: () => void;
  children?: React.ReactNode;
}) {
  const [code, setCode] = useState("");
  const mutation = useMutation({
    mutationFn: () => authApi.accountMfaConfirm(factor.id, code.trim()),
    onSuccess: onConfirmed,
  });
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            mutation.mutate();
          }}
        >
          {children}
          <Field>
            <FieldLabel htmlFor={`factor-code-${factor.id}`}>Verification code</FieldLabel>
            <Input
              id={`factor-code-${factor.id}`}
              inputMode="numeric"
              autoComplete="one-time-code"
              value={code}
              onChange={(event) => setCode(event.target.value)}
              required
            />
          </Field>
          {mutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : "Unable to verify code."}
            </p>
          )}
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Verifying..." : "Add factor"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
