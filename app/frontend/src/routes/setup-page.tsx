import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { Navigate, useNavigate } from "react-router-dom";
import { ApiError } from "@/api/client";
import {
  getSystemInfo,
  submitAdminSetup,
  submitDatabaseSetup,
  systemInfoQueryKey,
  type SetupStage,
  type SystemInfo,
} from "@/api/system";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

type DatabaseFormState = {
  host: string;
  port: string;
  dbName: string;
  username: string;
  password: string;
  schema: string;
};

type AdminFormState = {
  email: string;
  password: string;
  confirmPassword: string;
};

const stageCopy: Record<
  SetupStage,
  { eyebrow: string; title: string; description: string }
> = {
  database: {
    eyebrow: "Step 1 of 2",
    title: "Connect the control database",
    description:
      "Provide the PostgreSQL connection that the control API should initialize and persist to config.",
  },
  admin: {
    eyebrow: "Step 2 of 2",
    title: "Create the initial administrator",
    description:
      "Create the first admin account that will sign in to the ready-mode console.",
  },
};

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }

  return fallback;
}

function ProgressCard({ stage }: { stage: SetupStage }) {
  const steps = [
    {
      title: "Database",
      description: "Persist runtime database settings and initialize the schema.",
      state: stage === "database" ? "current" : "done",
    },
    {
      title: "Admin",
      description: "Create the first administrator account for the console.",
      state: stage === "admin" ? "current" : "upcoming",
    },
    {
      title: "Ready",
      description: "Return to the projects console and continue through normal sign-in.",
      state: "upcoming",
    },
  ] as const;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Setup progress</CardTitle>
        <CardDescription>
          The console will stay here until the backend reports that initialization is complete.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        {steps.map((step, index) => (
          <div className="rounded-lg border bg-background p-4" key={step.title}>
            <p className="text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
              {step.state === "current"
                ? "Current"
                : step.state === "done"
                  ? "Completed"
                  : `Step ${index + 1}`}
            </p>
            <p className="mt-2 font-medium">{step.title}</p>
            <p className="mt-1 text-sm text-muted-foreground">{step.description}</p>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

function SetupStateError({
  title,
  description,
  action,
}: {
  title: string;
  description: string;
  action?: React.ReactNode;
}) {
  return (
    <main className="flex min-h-screen items-center justify-center bg-muted/30 p-6">
      <Card className="w-full max-w-lg">
        <CardHeader>
          <CardTitle>{title}</CardTitle>
          <CardDescription>{description}</CardDescription>
        </CardHeader>
        {action ? <CardContent>{action}</CardContent> : null}
      </Card>
    </main>
  );
}

export function SetupPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: systemInfoQueryKey,
    queryFn: getSystemInfo,
  });

  const [databaseForm, setDatabaseForm] = React.useState<DatabaseFormState>({
    host: "127.0.0.1",
    port: "5432",
    dbName: "",
    username: "",
    password: "",
    schema: "public",
  });
  const [adminForm, setAdminForm] = React.useState<AdminFormState>({
    email: "",
    password: "",
    confirmPassword: "",
  });
  const [databaseValidationError, setDatabaseValidationError] = React.useState<string | null>(null);
  const [adminValidationError, setAdminValidationError] = React.useState<string | null>(null);

  const refreshSystemInfo = React.useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: systemInfoQueryKey });
  }, [queryClient]);

  const databaseMutation = useMutation({
    mutationFn: () =>
      submitDatabaseSetup({
        host: databaseForm.host.trim(),
        port: Number.parseInt(databaseForm.port, 10),
        db_name: databaseForm.dbName.trim(),
        user: databaseForm.username.trim(),
        password: databaseForm.password,
        schema: databaseForm.schema.trim() || "public",
      }),
    onSuccess: refreshSystemInfo,
  });

  const adminMutation = useMutation({
    mutationFn: () =>
      submitAdminSetup({
        email: adminForm.email.trim(),
        password: adminForm.password,
      }),
    onSuccess: async () => {
      await refreshSystemInfo();
      const systemInfo = await queryClient.fetchQuery<SystemInfo>({
        queryKey: systemInfoQueryKey,
        queryFn: getSystemInfo,
      });

      if (systemInfo.mode === "ready") {
        const email = encodeURIComponent(adminForm.email.trim());
        await navigate(`/login?redirect=%2Fprojects&email=${email}`, { replace: true });
      }
    },
  });

  React.useEffect(() => {
    setDatabaseValidationError(null);
    setAdminValidationError(null);
    databaseMutation.reset();
    adminMutation.reset();
  }, [query.data?.mode, query.data?.mode === "setup" ? query.data.stage : "ready"]);

  if (query.isPending) {
    return (
      <SetupStateError
        title="Loading setup"
        description="Checking which initialization step the backend needs next."
      />
    );
  }

  if (query.isError) {
    return (
      <SetupStateError
        title="Unable to load setup"
        description="The frontend could not determine the current setup stage."
        action={
          <Button onClick={() => void query.refetch()} type="button" variant="outline">
            Retry
          </Button>
        }
      />
    );
  }

  if (query.data.mode === "ready") {
    return <Navigate replace to="/projects" />;
  }

  const stage = query.data.stage;
  const copy = stageCopy[stage];
  const activeError =
    stage === "database"
      ? databaseValidationError ??
        (databaseMutation.isError
          ? errorMessage(databaseMutation.error, "Unable to save database settings")
          : null)
      : adminValidationError ??
        (adminMutation.isError
          ? errorMessage(adminMutation.error, "Unable to create the initial administrator")
          : null);

  return (
    <main className="min-h-screen bg-muted/30 px-6 py-10">
      <div className="mx-auto grid max-w-6xl gap-6 lg:grid-cols-[minmax(0,2.2fr)_minmax(280px,1fr)]">
        <div className="space-y-6">
          <Card>
            <CardHeader>
              <CardDescription>{copy.eyebrow}</CardDescription>
              <CardTitle>{copy.title}</CardTitle>
              <CardDescription>{copy.description}</CardDescription>
            </CardHeader>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>
                {stage === "database"
                  ? "Database connection"
                  : "Initial administrator"}
              </CardTitle>
              <CardDescription>
                {stage === "database"
                  ? "These settings are written to the API config and used to initialize the runtime database."
                  : "This account becomes the first administrator for the ready-mode console."}
              </CardDescription>
            </CardHeader>
            <CardContent>
              {activeError ? (
                <Alert className="mb-5" variant="destructive">
                  <AlertTitle>
                    {stage === "database" ? "Database setup failed" : "Admin setup failed"}
                  </AlertTitle>
                  <AlertDescription>{activeError}</AlertDescription>
                </Alert>
              ) : null}

              {stage === "database" ? (
                <form
                  className="space-y-5"
                  onSubmit={(event) => {
                    event.preventDefault();
                    const port = Number.parseInt(databaseForm.port, 10);

                    if (!databaseForm.host.trim()) {
                      setDatabaseValidationError("Host is required");
                      return;
                    }

                    if (!Number.isInteger(port) || port <= 0) {
                      setDatabaseValidationError("Port must be a positive integer");
                      return;
                    }

                    if (!databaseForm.dbName.trim()) {
                      setDatabaseValidationError("Database name is required");
                      return;
                    }

                    if (!databaseForm.username.trim()) {
                      setDatabaseValidationError("Username is required");
                      return;
                    }

                    if (!databaseForm.password) {
                      setDatabaseValidationError("Password is required");
                      return;
                    }

                    setDatabaseValidationError(null);
                    databaseMutation.mutate();
                  }}
                >
                  <div className="grid gap-5 md:grid-cols-2">
                    <div className="space-y-2">
                      <Label htmlFor="setup-host">Host</Label>
                      <Input
                        disabled={databaseMutation.isPending}
                        id="setup-host"
                        onChange={(event) =>
                          setDatabaseForm((current) => ({
                            ...current,
                            host: event.target.value,
                          }))}
                        value={databaseForm.host}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="setup-port">Port</Label>
                      <Input
                        disabled={databaseMutation.isPending}
                        id="setup-port"
                        inputMode="numeric"
                        onChange={(event) =>
                          setDatabaseForm((current) => ({
                            ...current,
                            port: event.target.value,
                          }))}
                        value={databaseForm.port}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="setup-db-name">Database Name</Label>
                      <Input
                        disabled={databaseMutation.isPending}
                        id="setup-db-name"
                        onChange={(event) =>
                          setDatabaseForm((current) => ({
                            ...current,
                            dbName: event.target.value,
                          }))}
                        placeholder="grass_worker"
                        value={databaseForm.dbName}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="setup-username">Username</Label>
                      <Input
                        autoComplete="username"
                        disabled={databaseMutation.isPending}
                        id="setup-username"
                        onChange={(event) =>
                          setDatabaseForm((current) => ({
                            ...current,
                            username: event.target.value,
                          }))}
                        placeholder="postgres"
                        value={databaseForm.username}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="setup-password">Password</Label>
                      <Input
                        autoComplete="current-password"
                        disabled={databaseMutation.isPending}
                        id="setup-password"
                        onChange={(event) =>
                          setDatabaseForm((current) => ({
                            ...current,
                            password: event.target.value,
                          }))}
                        type="password"
                        value={databaseForm.password}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="setup-schema">Schema</Label>
                      <Input
                        disabled={databaseMutation.isPending}
                        id="setup-schema"
                        onChange={(event) =>
                          setDatabaseForm((current) => ({
                            ...current,
                            schema: event.target.value,
                          }))}
                        value={databaseForm.schema}
                      />
                    </div>
                  </div>
                  <Button
                    className="w-full"
                    disabled={databaseMutation.isPending}
                    type="submit"
                  >
                    {databaseMutation.isPending ? "Saving..." : "Save and continue"}
                  </Button>
                </form>
              ) : (
                <form
                  className="space-y-5"
                  onSubmit={(event) => {
                    event.preventDefault();

                    if (!adminForm.email.trim()) {
                      setAdminValidationError("Email is required");
                      return;
                    }

                    if (!adminForm.password) {
                      setAdminValidationError("Password is required");
                      return;
                    }

                    if (adminForm.password !== adminForm.confirmPassword) {
                      setAdminValidationError("Passwords do not match");
                      return;
                    }

                    setAdminValidationError(null);
                    adminMutation.mutate();
                  }}
                >
                  <div className="space-y-2">
                    <Label htmlFor="setup-admin-email">Email</Label>
                    <Input
                      autoComplete="username"
                      disabled={adminMutation.isPending}
                      id="setup-admin-email"
                      onChange={(event) =>
                        setAdminForm((current) => ({
                          ...current,
                          email: event.target.value,
                        }))}
                      placeholder="admin@example.com"
                      value={adminForm.email}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="setup-admin-password">Password</Label>
                    <Input
                      autoComplete="new-password"
                      disabled={adminMutation.isPending}
                      id="setup-admin-password"
                      onChange={(event) =>
                        setAdminForm((current) => ({
                          ...current,
                          password: event.target.value,
                        }))}
                      type="password"
                      value={adminForm.password}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="setup-admin-confirm-password">Confirm password</Label>
                    <Input
                      autoComplete="new-password"
                      disabled={adminMutation.isPending}
                      id="setup-admin-confirm-password"
                      onChange={(event) =>
                        setAdminForm((current) => ({
                          ...current,
                          confirmPassword: event.target.value,
                        }))}
                      type="password"
                      value={adminForm.confirmPassword}
                    />
                  </div>
                  <Button
                    className="w-full"
                    disabled={adminMutation.isPending}
                    type="submit"
                  >
                    {adminMutation.isPending ? "Finishing..." : "Finish setup"}
                  </Button>
                </form>
              )}
            </CardContent>
          </Card>
        </div>

        <ProgressCard stage={stage} />
      </div>
    </main>
  );
}
