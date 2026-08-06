import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MailIcon, SmartphoneIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { QRCodeSVG } from "qrcode.react";

import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { showErrorToast } from "@/lib/toast";
import { authApi, isAuthResponse, type MfaFactor, type TotpEnrollment } from "./auth.api";
import { useAuth } from "./auth-context";

function challengeToken(): string {
  return new URLSearchParams(window.location.hash.replace(/^#/, "")).get("challenge") ?? "";
}

export function MfaRoute() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { completeMfa } = useAuth();
  const token = useMemo(challengeToken, []);
  const challenge = useQuery({
    queryKey: ["auth", "mfa", token],
    queryFn: () => authApi.mfaChallenge(token),
    enabled: Boolean(token),
  });
  const [factor, setFactor] = useState<MfaFactor | null>(null);
  const [totpEnrollment, setTotpEnrollment] = useState<TotpEnrollment | null>(null);
  const [code, setCode] = useState("");

  useEffect(() => {
    if (!token) showErrorToast(new Error("The MFA challenge is missing."), "mfa-missing");
  }, [token]);

  const startTotp = useMutation({
    mutationFn: () => authApi.mfaTotpStart(token),
    onSuccess: (enrollment) => {
      setTotpEnrollment(enrollment);
      setFactor(enrollment.factor);
    },
  });
  const sendEmail = useMutation({
    mutationFn: (selected?: MfaFactor) => authApi.mfaEmailSend(token, selected?.id),
    onSuccess: ({ factor: selected }) => {
      setFactor(selected);
      setTotpEnrollment(null);
    },
  });
  const verify = useMutation({
    mutationFn: async () => {
      if (!factor) throw new Error("Choose an MFA factor.");
      return completeMfa(token, factor.id, code.trim());
    },
    onSuccess: (result) => {
      if (!isAuthResponse(result)) {
        queryClient.setQueryData(["auth", "mfa", token], result);
        setFactor(null);
        setTotpEnrollment(null);
        setCode("");
        return;
      }
      const returnTo = challenge.data?.return_to ?? "/";
      if (returnTo.startsWith("/api/")) window.location.assign(returnTo);
      else navigate(returnTo, { replace: true });
    },
  });

  if (!token) return null;
  if (challenge.isLoading) return <MfaMessage message="Loading verification..." />;
  if (challenge.isError || !challenge.data) return null;

  const enrollment = challenge.data.mfa_enrollment_required;
  const factors = challenge.data.factors;
  const allowed = challenge.data.allowed_factors;

  return (
    <div className="flex w-full max-w-sm flex-col gap-6">
      <div className="space-y-2 text-center">
        <h1 className="text-xl font-semibold">
          {enrollment ? "Add multi-factor authentication" : "Verify your sign-in"}
        </h1>
      </div>
      <Card>
        <CardContent className="grid gap-5">
          {!factor && (
            <div className="grid gap-3">
              {(enrollment
                ? allowed.includes("totp") && !factors.some((item) => item.kind === "totp")
                : factors.some((item) => item.kind === "totp")) && (
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => {
                    if (enrollment) startTotp.mutate();
                    else setFactor(factors.find((item) => item.kind === "totp") ?? null);
                  }}
                >
                  <SmartphoneIcon /> Authenticator app
                </Button>
              )}
              {(enrollment
                ? allowed.includes("email") && !factors.some((item) => item.kind === "email")
                : factors.some((item) => item.kind === "email")) && (
                <Button
                  type="button"
                  variant="outline"
                  onClick={() =>
                    sendEmail.mutate(
                      enrollment ? undefined : factors.find((item) => item.kind === "email"),
                    )
                  }
                >
                  <MailIcon /> Email code
                </Button>
              )}
            </div>
          )}

          {totpEnrollment && (
            <div className="grid justify-items-center gap-3">
              <div className="bg-white p-3">
                <QRCodeSVG value={totpEnrollment.otpauth_uri} size={176} />
              </div>
              <code className="max-w-full break-all rounded bg-muted px-2 py-1 text-xs">
                {totpEnrollment.secret}
              </code>
            </div>
          )}

          {factor && (
            <form
              className="grid gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                verify.mutate();
              }}
            >
              <div className="grid gap-2">
                <Label htmlFor="mfa-code">Verification code</Label>
                <Input
                  id="mfa-code"
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  value={code}
                  onChange={(event) => setCode(event.target.value)}
                  required
                />
              </div>
              <div className="flex gap-2">
                <Button type="button" variant="outline" onClick={() => setFactor(null)}>
                  Back
                </Button>
                <Button type="submit" className="flex-1" disabled={verify.isPending}>
                  {verify.isPending ? "Verifying..." : "Continue"}
                </Button>
              </div>
            </form>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function MfaMessage({ message }: { message: string }) {
  return (
    <div className="w-full max-w-sm">
      <Card>
        <CardContent>
          <p role="status" className="text-sm text-muted-foreground">
            {message}
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
