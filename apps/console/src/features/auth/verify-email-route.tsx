import { useEffect, useRef, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router";

import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { showErrorToast } from "@/lib/toast";
import { authApi } from "./auth.api";
import { authHref, safeLocalReturnTo } from "./auth-continuation";
import { useAuth } from "./auth-context";

export function VerifyEmailRoute() {
  const location = useLocation();
  const navigate = useNavigate();
  const { verifyEmail } = useAuth();
  const query = new URLSearchParams(location.search);
  const token = query.get("token") ?? "";
  const returnTo = safeLocalReturnTo(query.get("return_to"));
  const [email, setEmail] = useState(query.get("email") ?? "");
  const [pending, setPending] = useState(false);
  const [sent, setSent] = useState(false);
  const started = useRef(false);

  useEffect(() => {
    if (!token || started.current) return;
    started.current = true;
    setPending(true);
    verifyEmail(token)
      .then(() => navigate(returnTo ?? "/", { replace: true }))
      .catch(showErrorToast)
      .finally(() => setPending(false));
  }, [navigate, returnTo, token, verifyEmail]);

  return (
    <div className="flex w-full max-w-sm flex-col gap-6">
      <div className="space-y-2 text-center">
        <h1 className="text-xl font-semibold">Verify your email</h1>
        <Link to={authHref("/login", returnTo)} className="text-sm underline underline-offset-4">
          Back to login
        </Link>
      </div>
      <Card>
        <CardContent>
          {pending ? (
            <p role="status" className="text-sm text-muted-foreground">
              Verifying...
            </p>
          ) : (
            <form
              className="grid gap-5"
              onSubmit={async (event) => {
                event.preventDefault();
                setPending(true);
                try {
                  await authApi.resendVerification(email.trim(), returnTo ?? undefined);
                  setSent(true);
                } catch (cause) {
                  showErrorToast(cause);
                } finally {
                  setPending(false);
                }
              }}
            >
              <div className="grid gap-2">
                <Label htmlFor="verification-email">Email</Label>
                <Input
                  id="verification-email"
                  type="email"
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                  required
                />
              </div>
              {sent && <p className="text-sm text-muted-foreground">Verification email sent.</p>}
              <Button type="submit" disabled={pending}>
                {pending ? "Sending..." : "Resend verification"}
              </Button>
            </form>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
