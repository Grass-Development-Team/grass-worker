import { useMutation } from "@tanstack/react-query";
import { Navigate, useNavigate, useSearchParams } from "react-router";

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
import { useTeam } from "./team-context";
import { teamsApi } from "./teams.api";

export function AcceptInvitationRoute() {
  const [params] = useSearchParams();
  const token = params.get("token")?.trim() ?? "";
  const navigate = useNavigate();
  const { refreshTeams, selectTeam } = useTeam();
  const accept = useMutation({
    mutationFn: () => teamsApi.acceptInvitation(token),
    onSuccess: async ({ member }) => {
      await refreshTeams();
      selectTeam(member.team_id);
      navigate("/", { replace: true });
    },
  });
  if (!token) return <Navigate to="/" replace />;
  return (
    <div className="mx-auto flex w-full max-w-lg flex-1 items-center">
      <Card className="w-full">
        <CardHeader>
          <CardTitle>Accept team invitation</CardTitle>
          <CardDescription>Join the team associated with this invitation.</CardDescription>
        </CardHeader>
        <CardContent>
          {accept.error && <p className="text-sm text-destructive">{accept.error.message}</p>}
        </CardContent>
        <CardFooter>
          <Button disabled={accept.isPending} onClick={() => accept.mutate()}>
            {accept.isPending && <Spinner data-icon="inline-start" />}Accept invitation
          </Button>
        </CardFooter>
      </Card>
    </div>
  );
}
