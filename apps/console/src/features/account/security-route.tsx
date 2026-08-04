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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
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

  const formatTimestamp = (value: string | null | undefined, fallback: string) => {
    if (!value) return fallback;
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? "Unknown date" : date.toLocaleString();
  };

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <PasswordForm policy={security.data.password_policy} />
      <SettingsCard
        title="Multi-factor authentication"
        description="Manage the verification methods used for future sign-ins."
      >
        <div className="grid gap-4">
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border bg-muted/30 px-4 py-3">
            <div>
              <p className="text-sm font-medium">
                {security.data.factors.length} method{security.data.factors.length === 1 ? "" : "s"}{" "}
                configured
              </p>
              <p className="text-xs text-muted-foreground">
                {security.data.mfa_requirements.minimum_factors > 0
                  ? `At least ${security.data.mfa_requirements.minimum_factors} method${security.data.mfa_requirements.minimum_factors === 1 ? "" : "s"} required for your account.`
                  : "Methods are optional unless required by the platform."}
              </p>
            </div>
            {security.data.mfa_requirements.required_factors.length > 0 && (
              <Badge variant="secondary">Policy enforced</Badge>
            )}
          </div>
          <div className="overflow-x-auto rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Method</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Added</TableHead>
                  <TableHead>Last used</TableHead>
                  <TableHead>Requirement</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {security.data.allowed_factors.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={6}
                      className="h-20 text-center text-sm text-muted-foreground"
                    >
                      No MFA methods are available.
                    </TableCell>
                  </TableRow>
                ) : (
                  security.data.allowed_factors.map((kind) => {
                    const factor = security.data.factors.find(
                      (candidate) => candidate.kind === kind,
                    );
                    const required = security.data.mfa_requirements.required_factors.includes(kind);
                    const emailUnavailable =
                      kind === "email" &&
                      (!security.data.mail_available || !security.data.email_verified);
                    return (
                      <TableRow key={kind}>
                        <TableCell>
                          <div className="flex items-center gap-2">
                            {kind === "totp" ? (
                              <SmartphoneIcon className="size-4 shrink-0" />
                            ) : (
                              <MailIcon className="size-4 shrink-0" />
                            )}
                            <span className="font-medium">
                              {kind === "totp" ? "Authenticator app" : "Email code"}
                            </span>
                          </div>
                        </TableCell>
                        <TableCell>
                          {factor ? (
                            <Badge variant={factor.verified ? "success" : "secondary"}>
                              {factor.verified ? "Verified" : "Pending"}
                            </Badge>
                          ) : (
                            <Badge variant="outline">Not configured</Badge>
                          )}
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatTimestamp(factor?.created_at, "Not configured")}
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatTimestamp(factor?.last_used_at, "Never")}
                        </TableCell>
                        <TableCell>
                          {required ? (
                            <Badge variant="secondary">Required</Badge>
                          ) : (
                            <span className="text-sm text-muted-foreground">Optional</span>
                          )}
                        </TableCell>
                        <TableCell className="text-right">
                          {factor ? (
                            <Button
                              type="button"
                              size="icon-sm"
                              variant="ghost"
                              aria-label={`Remove ${kind} factor`}
                              onClick={() => remove.mutate(factor.id)}
                              disabled={remove.isPending}
                            >
                              <Trash2Icon />
                            </Button>
                          ) : (
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              onClick={() =>
                                kind === "totp" ? startTotp.mutate() : startEmail.mutate()
                              }
                              disabled={
                                emailUnavailable || startTotp.isPending || startEmail.isPending
                              }
                            >
                              <PlusIcon /> Add
                            </Button>
                          )}
                        </TableCell>
                      </TableRow>
                    );
                  })
                )}
              </TableBody>
            </Table>
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
