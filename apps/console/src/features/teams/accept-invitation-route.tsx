import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";

import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import { useAuth } from "@/features/auth/auth-context";
import { usePageTitle } from "@/features/branding/branding-context";
import { showErrorToast } from "@/lib/toast";
import { ACTIVE_TEAM_STORAGE_KEY } from "./team-context";
import { teamsApi } from "./teams.api";

const invitationStateMessage = (status: string) => {
  switch (status) {
    case "email_mismatch":
      return "This invitation was sent to a different email address.";
    case "expired":
      return "This invitation has expired.";
    case "accepted":
      return "This invitation has already been used.";
    case "revoked":
      return "This invitation has been revoked.";
    default:
      return null;
  }
};

const invitationHref = (path: "/login" | "/signup", token: string) =>
  `${path}?${new URLSearchParams({ return_to: `/invitations/accept?token=${token}` })}`;

const roleLabel = (role: string) => `${role.charAt(0).toUpperCase()}${role.slice(1)}`;

export function AcceptInvitationRoute() {
  usePageTitle("Invitation");
  const [params] = useSearchParams();
  const token = params.get("token")?.trim() ?? "";
  const navigate = useNavigate();
  const { user } = useAuth();
  const preflight = useQuery({
    queryKey: ["invitation-preflight", token, user?.id ?? "anonymous"],
    queryFn: () => teamsApi.preflightInvitation(token),
    enabled: Boolean(token),
    retry: false,
  });
  const accept = useMutation({
    mutationFn: () => teamsApi.acceptInvitation(token),
    onSuccess: ({ member }) => {
      localStorage.setItem(ACTIVE_TEAM_STORAGE_KEY, member.team_id);
      navigate("/", { replace: true });
    },
  });

  const stateMessage = preflight.data && invitationStateMessage(preflight.data.status);

  useEffect(() => {
    if (!token) {
      showErrorToast(new Error("The invitation link is missing its token."), "invitation-missing");
    } else if (stateMessage) {
      showErrorToast(new Error(stateMessage), `invitation-state:${preflight.data?.status}`);
    }
  }, [preflight.data?.status, stateMessage, token]);

  return (
    <div className="mx-auto flex w-full max-w-lg flex-1 items-center">
      <Card className="w-full">
        <CardHeader>
          <CardTitle>Team invitation</CardTitle>
          <CardDescription>Review the invitation before joining.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {!token ? null : preflight.isLoading ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Spinner /> Checking invitation…
            </div>
          ) : preflight.data ? (
            <>
              <dl className="grid grid-cols-[7rem_1fr] gap-x-4 gap-y-3 text-sm">
                <dt className="text-muted-foreground">Team</dt>
                <dd className="font-medium">{preflight.data.team.name}</dd>
                <dt className="text-muted-foreground">Role</dt>
                <dd>{roleLabel(preflight.data.role)}</dd>
                <dt className="text-muted-foreground">Expires</dt>
                <dd>
                  {new Intl.DateTimeFormat("en", {
                    dateStyle: "medium",
                    timeStyle: "short",
                  }).format(new Date(preflight.data.expires_at))}
                </dd>
              </dl>
            </>
          ) : null}
        </CardContent>
        {preflight.data && !stateMessage && (
          <CardFooter className="gap-2">
            {!user ? (
              <>
                <Button asChild>
                  <Link to={invitationHref("/login", token)}>Log in</Link>
                </Button>
                <Button asChild variant="outline">
                  <Link to={invitationHref("/signup", token)}>Create account</Link>
                </Button>
              </>
            ) : preflight.data.can_accept ? (
              <Button disabled={accept.isPending} onClick={() => accept.mutate()}>
                {accept.isPending && <Spinner data-icon="inline-start" />}Accept invitation
              </Button>
            ) : null}
          </CardFooter>
        )}
      </Card>
    </div>
  );
}
