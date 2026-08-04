import { useState } from "react";
import { Link, useLocation, useNavigate } from "react-router";

import { SiteLogo } from "@/components/site-logo";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useBranding } from "@/features/branding/branding-context";
import { useAuth } from "./auth-context";
import { isAuthResponse } from "./auth.api";
import { ProviderButtons, useAuthConfiguration } from "./provider-buttons";

export function previewAuthorizationContinuation(
  search: string,
  origin = window.location.origin,
): string | null {
  const value = new URLSearchParams(search).get("continue");
  if (
    !value ||
    value.length > 4096 ||
    !value.startsWith("/api/v1/preview/authorize?") ||
    value.startsWith("//") ||
    value.includes("\\") ||
    [...value].some((character) => {
      const code = character.charCodeAt(0);
      return code <= 0x1f || code === 0x7f;
    })
  ) {
    return null;
  }

  const destination = new URL(value, origin);
  if (
    destination.origin !== origin ||
    destination.pathname !== "/api/v1/preview/authorize" ||
    destination.hash ||
    destination.searchParams.getAll("state").length !== 1 ||
    !destination.searchParams.get("state")
  ) {
    return null;
  }
  return `${destination.pathname}${destination.search}`;
}

type LoginFormProps = React.ComponentPropsWithoutRef<"div"> & {
  documentNavigate?: (destination: string) => void;
};

export function LoginForm({
  className,
  documentNavigate = (destination) => window.location.assign(destination),
  ...props
}: LoginFormProps) {
  const { siteName } = useBranding();
  const navigate = useNavigate();
  const location = useLocation();
  const previewContinuation = previewAuthorizationContinuation(location.search);
  const from = (location.state as { from?: string } | null)?.from;
  const redirectedInvitationToken = (() => {
    if (!from?.startsWith("/")) return null;
    const redirectUrl = new URL(from, window.location.origin);
    return redirectUrl.pathname === "/invitations/accept"
      ? redirectUrl.searchParams.get("token")
      : null;
  })();
  const invitationToken =
    new URLSearchParams(location.search).get("invitation_token") ?? redirectedInvitationToken;
  const signupHref = invitationToken
    ? `/signup?${new URLSearchParams({ invitation_token: invitationToken })}`
    : "/signup";
  const { login } = useAuth();
  const configuration = useAuthConfiguration();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!email.trim() || !password.trim()) {
      setError("Please enter your email and password.");
      return;
    }

    setIsSubmitting(true);
    try {
      const destination =
        from ??
        (invitationToken
          ? `/invitations/accept?${new URLSearchParams({ token: invitationToken })}`
          : "/");
      const result = await login(email.trim(), password, previewContinuation ?? destination);
      if (!isAuthResponse(result)) {
        navigate(`/mfa#challenge=${encodeURIComponent(result.challenge_token)}`, {
          replace: true,
        });
        return;
      }
      if (previewContinuation) {
        documentNavigate(previewContinuation);
        return;
      }
      navigate(destination, { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Login failed");
      setIsSubmitting(false);
    }
  };

  return (
    <div className={cn("flex flex-col gap-6", className)} {...props}>
      <div className="flex flex-col items-center gap-2">
        <div className="flex size-10 items-center justify-center">
          <SiteLogo className="size-5" />
          <span className="sr-only">{siteName}</span>
        </div>
        <h1 className="text-xl font-semibold">Welcome to {siteName}</h1>
        <div className="text-center text-sm text-muted-foreground">
          Don&apos;t have an account?{" "}
          <Link to={signupHref} className="text-foreground underline underline-offset-4">
            Sign up
          </Link>
        </div>
      </div>
      <Card>
        <CardContent>
          <form onSubmit={handleSubmit} className="flex flex-col gap-6">
            <div className="grid gap-2">
              <Label htmlFor="email">Email</Label>
              <Input
                id="email"
                type="email"
                placeholder="you@example.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
              />
            </div>
            <div className="grid gap-2">
              <div className="flex items-center justify-between gap-3">
                <Label htmlFor="password">Password</Label>
                {configuration?.password_recovery_available && (
                  <Link to="/forgot-password" className="text-xs underline underline-offset-4">
                    Forgot password?
                  </Link>
                )}
              </div>
              <Input
                id="password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </div>
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            <Button type="submit" className="w-full" disabled={isSubmitting}>
              {isSubmitting ? "Signing in..." : "Login"}
            </Button>
          </form>
        </CardContent>
      </Card>
      <ProviderButtons returnTo={previewContinuation ?? from} invitationToken={invitationToken} />
    </div>
  );
}
