import { useState } from "react";
import { Link, useLocation, useNavigate } from "react-router";

import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { authApi } from "./auth.api";
import { useAuthConfiguration } from "./provider-buttons";

export function ResetPasswordRoute() {
  const location = useLocation();
  const navigate = useNavigate();
  const token = new URLSearchParams(location.search).get("token") ?? "";
  const configuration = useAuthConfiguration();
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  return (
    <div className="flex w-full max-w-sm flex-col gap-6">
      <div className="space-y-2 text-center">
        <h1 className="text-xl font-semibold">Choose a new password</h1>
        <Link to="/login" className="text-sm underline underline-offset-4">
          Back to login
        </Link>
      </div>
      <Card>
        <CardContent>
          <form
            className="grid gap-5"
            onSubmit={async (event) => {
              event.preventDefault();
              if (!token) {
                setError("The reset link is missing its token.");
                return;
              }
              if (password !== confirm) {
                setError("Passwords do not match.");
                return;
              }
              setPending(true);
              setError(null);
              try {
                await authApi.resetPassword(token, password);
                navigate("/login", { replace: true });
              } catch (cause) {
                setError(cause instanceof Error ? cause.message : "Unable to reset the password.");
              } finally {
                setPending(false);
              }
            }}
          >
            <div className="grid gap-2">
              <Label htmlFor="reset-password">New password</Label>
              <Input
                id="reset-password"
                type="password"
                autoComplete="new-password"
                minLength={configuration?.password_policy.min_length ?? 8}
                maxLength={configuration?.password_policy.max_length ?? 1024}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                required
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="reset-password-confirm">Confirm password</Label>
              <Input
                id="reset-password-confirm"
                type="password"
                autoComplete="new-password"
                value={confirm}
                onChange={(event) => setConfirm(event.target.value)}
                required
              />
            </div>
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
            <Button type="submit" disabled={pending}>
              {pending ? "Saving..." : "Save password"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
