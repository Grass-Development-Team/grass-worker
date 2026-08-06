import { useState } from "react";
import { Link } from "react-router";

import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { authApi } from "./auth.api";

export function ForgotPasswordRoute() {
  const [email, setEmail] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  return (
    <div className="flex w-full max-w-sm flex-col gap-6">
      <div className="space-y-2 text-center">
        <h1 className="text-xl font-semibold">Reset your password</h1>
        <Link to="/login" className="text-sm underline underline-offset-4">
          Back to login
        </Link>
      </div>
      <Card>
        <CardContent>
          {submitted ? (
            <p className="text-sm text-muted-foreground">
              If the account can receive mail, a reset link has been sent.
            </p>
          ) : (
            <form
              className="grid gap-5"
              onSubmit={async (event) => {
                event.preventDefault();
                setPending(true);
                setError(null);
                try {
                  await authApi.forgotPassword(email.trim());
                  setSubmitted(true);
                } catch (cause) {
                  setError(cause instanceof Error ? cause.message : "Unable to request a reset.");
                } finally {
                  setPending(false);
                }
              }}
            >
              <div className="grid gap-2">
                <Label htmlFor="forgot-email">Email</Label>
                <Input
                  id="forgot-email"
                  type="email"
                  autoComplete="email"
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                  required
                />
              </div>
              {error && (
                <p role="alert" className="text-sm text-destructive">
                  {error}
                </p>
              )}
              <Button type="submit" disabled={pending}>
                {pending ? "Sending..." : "Send reset link"}
              </Button>
            </form>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
