import { useState } from "react";
import { Link, useLocation, useNavigate } from "react-router";

import { SiteLogo } from "@/components/site-logo";
import { Alert, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { useBranding } from "@/features/branding/branding-context";
import { showErrorToast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { useAuth } from "./auth-context";
import { authHref, safeLocalReturnTo } from "./auth-continuation";
import { isAuthResponse } from "./auth.api";
import { ProviderButtons } from "./provider-buttons";
import { useAuthConfiguration } from "./provider-buttons";

export function SignupForm({ className, ...props }: React.ComponentPropsWithoutRef<"div">) {
  const { siteName } = useBranding();
  const navigate = useNavigate();
  const location = useLocation();
  const { register } = useAuth();
  const configuration = useAuthConfiguration();
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [registrationCode, setRegistrationCode] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const returnTo = safeLocalReturnTo(new URLSearchParams(location.search).get("return_to"));
  const signupPolicy = configuration?.signup_policy ?? "open";
  const loginHref = authHref("/login", returnTo);

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (
      !event.currentTarget.checkValidity() ||
      !email.trim() ||
      !displayName.trim() ||
      !password ||
      !confirmPassword
    ) {
      showErrorToast(new Error("Please complete all fields."));
      return;
    }
    if (password !== confirmPassword) {
      showErrorToast(new Error("Passwords do not match."));
      return;
    }

    setIsSubmitting(true);
    try {
      const result = await register({
        email: email.trim(),
        display_name: displayName.trim(),
        password,
        ...(returnTo ? { return_to: returnTo } : {}),
        ...(signupPolicy === "invite_only" && registrationCode.trim()
          ? { registration_code: registrationCode.trim() }
          : {}),
      });
      if (!isAuthResponse(result)) {
        navigate(
          `/verify-email?${new URLSearchParams({
            email: result.email,
            ...(returnTo ? { return_to: returnTo } : {}),
          })}`,
          {
            replace: true,
          },
        );
        return;
      }
      navigate(returnTo ?? "/", { replace: true });
    } catch (err) {
      showErrorToast(err);
      setIsSubmitting(false);
    }
  };

  return (
    <div className={cn("flex flex-col gap-6", className)} {...props}>
      <div className="flex flex-col items-center gap-2">
        <Link to="/" className="flex size-10 items-center justify-center">
          <SiteLogo className="size-5" />
          <span className="sr-only">{siteName}</span>
        </Link>
        <h1 className="text-xl font-semibold">
          {signupPolicy === "closed"
            ? `${siteName} registration`
            : `Create your ${siteName} account`}
        </h1>
        <div className="text-center text-sm text-muted-foreground">
          Already have an account?{" "}
          <Link to={loginHref} className="text-foreground underline underline-offset-4">
            Log in
          </Link>
        </div>
      </div>
      {signupPolicy === "closed" ? (
        <Alert>
          <AlertTitle>Registration is closed</AlertTitle>
        </Alert>
      ) : (
        <>
          <Card>
            <CardContent>
              <form onSubmit={handleSubmit} noValidate>
                <FieldGroup className="gap-4">
                  <Field className="gap-2">
                    <FieldLabel htmlFor="email">Email</FieldLabel>
                    <Input
                      id="email"
                      type="email"
                      autoComplete="email"
                      placeholder="m@example.com"
                      value={email}
                      onChange={(event) => setEmail(event.target.value)}
                      required
                    />
                  </Field>
                  <Field className="gap-2">
                    <FieldLabel htmlFor="display-name">Display name</FieldLabel>
                    <Input
                      id="display-name"
                      autoComplete="name"
                      value={displayName}
                      onChange={(event) => setDisplayName(event.target.value)}
                      required
                    />
                  </Field>
                  <Field className="gap-2">
                    <FieldLabel htmlFor="password">Password</FieldLabel>
                    <Input
                      id="password"
                      type="password"
                      autoComplete="new-password"
                      minLength={configuration?.password_policy.min_length ?? 8}
                      maxLength={configuration?.password_policy.max_length ?? 1024}
                      value={password}
                      onChange={(event) => setPassword(event.target.value)}
                      required
                    />
                  </Field>
                  <Field className="gap-2">
                    <FieldLabel htmlFor="confirm-password">Confirm password</FieldLabel>
                    <Input
                      id="confirm-password"
                      type="password"
                      autoComplete="new-password"
                      value={confirmPassword}
                      onChange={(event) => setConfirmPassword(event.target.value)}
                      required
                    />
                  </Field>
                  {signupPolicy === "invite_only" && (
                    <Field className="gap-2">
                      <FieldLabel htmlFor="registration-code">
                        Registration code (optional)
                      </FieldLabel>
                      <Input
                        id="registration-code"
                        autoComplete="one-time-code"
                        value={registrationCode}
                        onChange={(event) => setRegistrationCode(event.target.value)}
                      />
                    </Field>
                  )}
                  <Field>
                    <Button type="submit" className="w-full" disabled={isSubmitting}>
                      {isSubmitting ? "Creating account..." : "Create account"}
                    </Button>
                  </Field>
                </FieldGroup>
              </form>
            </CardContent>
          </Card>
          <ProviderButtons returnTo={returnTo ?? undefined} registrationCode={registrationCode} />
        </>
      )}
    </div>
  );
}
