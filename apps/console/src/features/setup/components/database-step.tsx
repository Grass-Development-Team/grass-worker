import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { AlertCircle, ArrowRight, Database } from "lucide-react";

import { setupApi } from "@/features/setup/setup.api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export function DatabaseStep({ onSuccess }: { onSuccess: () => void }) {
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [database, setDatabase] = useState("");
  const [error, setError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () => setupApi.configureDatabase(host, port, username, password, database),
    onSuccess,
    onError: (err: Error) => setError(err.message),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Database className="size-5" /> Configure Database
        </CardTitle>
        <CardDescription>Enter your PostgreSQL connection details to get started.</CardDescription>
      </CardHeader>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          setError(null);
          if (!host.trim() || !port.trim() || !username.trim() || !database.trim()) {
            setError("All fields are required.");
            return;
          }
          if (!/^\d+$/.test(port.trim())) {
            setError("Port must be a number.");
            return;
          }
          mutation.mutate();
        }}
      >
        <CardContent>
          <div className="grid gap-4">
            <div className="grid grid-cols-3 gap-3">
              <div className="col-span-2 grid gap-1.5">
                <Label htmlFor="db-host">Host</Label>
                <Input
                  id="db-host"
                  value={host}
                  onChange={(e) => setHost(e.target.value)}
                  placeholder="127.0.0.1"
                  required
                />
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="db-port">Port</Label>
                <Input
                  id="db-port"
                  value={port}
                  onChange={(e) => setPort(e.target.value)}
                  placeholder="5432"
                  required
                />
              </div>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="grid gap-1.5">
                <Label htmlFor="db-username">Username</Label>
                <Input
                  id="db-username"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder="postgres"
                  autoComplete="username"
                  required
                />
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="db-password">Password</Label>
                <Input
                  id="db-password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder="postgres"
                  autoComplete="current-password"
                  required
                />
              </div>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="db-name">Database</Label>
              <Input
                id="db-name"
                value={database}
                onChange={(e) => setDatabase(e.target.value)}
                placeholder="grass_worker"
                required
              />
            </div>
            {error && (
              <div className="flex items-center gap-2 text-sm text-destructive">
                <AlertCircle className="size-4" />
                {error}
              </div>
            )}
          </div>
        </CardContent>
        <CardFooter>
          <Button type="submit" className="w-full" disabled={mutation.isPending}>
            {mutation.isPending ? "Connecting..." : "Connect to Database"}
            {!mutation.isPending && <ArrowRight className="ml-2 size-4" />}
          </Button>
        </CardFooter>
      </form>
    </Card>
  );
}
