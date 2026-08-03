import { useState } from "react";
import { Link, useLocation, useNavigate } from "react-router";

import { SiteLogo } from "@/components/site-logo";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useBranding } from "@/features/branding/branding-context";
import { cn } from "@/lib/utils";
import { useAuth } from "./auth-context";

export function SignupForm({ className, ...props }: React.ComponentPropsWithoutRef<"div">) {
  const { siteName } = useBranding();
  const navigate = useNavigate();
  const location = useLocation();
  const { register } = useAuth();
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const invitationToken = new URLSearchParams(location.search).get("invitation_token") ?? undefined;
  const loginHref = invitationToken
    ? `/login?${new URLSearchParams({ invitation_token: invitationToken })}`
    : "/login";

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    setError(null);

    if (!email.trim() || !displayName.trim() || !password || !confirmPassword) {
      setError("Please complete all fields.");
      return;
    }
    if (password !== confirmPassword) {
      setError("Passwords do not match.");
      return;
    }

    setIsSubmitting(true);
    try {
      await register({
        email: email.trim(),
        display_name: displayName.trim(),
        password,
        ...(invitationToken ? { invitation_token: invitationToken } : {}),
      });
      navigate("/", { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Registration failed");
      setIsSubmitting(false);
    }
  };

  return (
    <div className={cn("flex flex-col gap-6", className)} {...props}>
      <div className="flex flex-col items-center gap-2">
        <Link
          to="/"
          className="bg-primary text-primary-foreground flex size-9 items-center justify-center rounded-lg"
        >
          <SiteLogo className="size-5" />
          <span className="sr-only">{siteName}</span>
        </Link>
        <h1 className="text-xl font-semibold">Create your {siteName} account</h1>
        <div className="text-center text-sm text-muted-foreground">
          Already have an account?{" "}
          <Link to={loginHref} className="text-foreground underline underline-offset-4">
            Log in
          </Link>
        </div>
      </div>
      <Card>
        <CardContent>
          <form onSubmit={handleSubmit} className="grid gap-4">
            <div className="grid gap-2">
              <Label htmlFor="email">Email</Label>
              <Input
                id="email"
                type="email"
                autoComplete="email"
                placeholder="m@example.com"
                value={email}
                onChange={(event) => setEmail(event.target.value)}
                required
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="display-name">Display name</Label>
              <Input
                id="display-name"
                autoComplete="name"
                value={displayName}
                onChange={(event) => setDisplayName(event.target.value)}
                required
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                required
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="confirm-password">Confirm password</Label>
              <Input
                id="confirm-password"
                type="password"
                autoComplete="new-password"
                value={confirmPassword}
                onChange={(event) => setConfirmPassword(event.target.value)}
                required
              />
            </div>
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            <Button type="submit" className="w-full" disabled={isSubmitting}>
              {isSubmitting ? "Creating account..." : "Create account"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
