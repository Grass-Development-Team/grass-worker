import { useState } from "react";
import { ActivityIcon } from "lucide-react";
import { Link, useLocation, useNavigate } from "react-router";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAuth } from "./auth-context";

export function LoginForm({ className, ...props }: React.ComponentPropsWithoutRef<"div">) {
  const navigate = useNavigate();
  const location = useLocation();
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
      await login(email.trim(), password);
      const destination =
        from ??
        (invitationToken
          ? `/invitations/accept?${new URLSearchParams({ token: invitationToken })}`
          : "/");
      navigate(destination, { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Login failed");
      setIsSubmitting(false);
    }
  };

  return (
    <div className={cn("flex flex-col gap-6", className)} {...props}>
      <div className="flex flex-col items-center gap-2">
        <div className="bg-primary text-primary-foreground flex size-9 items-center justify-center rounded-lg">
          <ActivityIcon className="size-5" />
          <span className="sr-only">Grass Worker</span>
        </div>
        <h1 className="text-xl font-semibold">Welcome to Grass Worker</h1>
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
              <Label htmlFor="password">Password</Label>
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
    </div>
  );
}
