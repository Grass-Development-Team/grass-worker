import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { Navigate, useNavigate, useSearchParams } from "react-router-dom";
import { currentUserQueryKey, getCurrentUser, login } from "@/api/auth";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

function safeRedirect(value: string | null) {
  if (!value) {
    return "/projects";
  }

  if (!value.startsWith("/") || value.startsWith("//")) {
    return "/projects";
  }

  return value;
}

export function LoginPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [searchParams] = useSearchParams();
  const redirect = safeRedirect(searchParams.get("redirect"));
  const requestedEmail = searchParams.get("email") ?? "";
  const { data: currentUser, isPending } = useQuery({
    queryKey: currentUserQueryKey,
    queryFn: getCurrentUser,
  });
  const [email, setEmail] = React.useState(requestedEmail);
  const [password, setPassword] = React.useState("");

  React.useEffect(() => {
    setEmail(requestedEmail);
  }, [requestedEmail]);

  const mutation = useMutation({
    mutationFn: () => login(email, password),
    onSuccess: async (user) => {
      queryClient.setQueryData(currentUserQueryKey, user);
      await navigate(redirect, { replace: true });
    },
  });

  if (!isPending && currentUser) {
    return <Navigate replace to={redirect} />;
  }

  return (
    <main className="flex min-h-screen items-center justify-center bg-muted/30 px-6 py-16">
      <Card className="w-full max-w-md shadow-sm">
        <CardHeader>
          <CardTitle>Sign in to the admin console</CardTitle>
          <CardDescription>
            Use the initial admin account created during setup to enter the ready
            mode console.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="space-y-5"
            onSubmit={(event) => {
              event.preventDefault();
              mutation.mutate();
            }}
          >
            <div className="space-y-2">
              <Label htmlFor="email">Email</Label>
              <Input
                autoComplete="username"
                id="email"
                onChange={(event) => setEmail(event.target.value)}
                placeholder="admin@example.com"
                value={email}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">Password</Label>
              <Input
                autoComplete="current-password"
                id="password"
                onChange={(event) => setPassword(event.target.value)}
                placeholder="Enter your password"
                type="password"
                value={password}
              />
            </div>
            {mutation.isError ? (
              <Alert variant="destructive">
                <AlertTitle>Sign-in failed</AlertTitle>
                <AlertDescription>邮箱或密码错误</AlertDescription>
              </Alert>
            ) : null}
            <Button className="w-full" disabled={mutation.isPending} type="submit">
              {mutation.isPending ? "Signing in..." : "Sign in"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </main>
  );
}
