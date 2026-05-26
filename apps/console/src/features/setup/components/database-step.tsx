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
  const [url, setUrl] = useState("postgres://postgres:postgres@127.0.0.1:5432/grass_worker");
  const [error, setError] = useState<string | null>(null);
  const mutation = useMutation({
    mutationFn: setupApi.configureDatabase,
    onSuccess,
    onError: (err: Error) => setError(err.message),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Database className="size-5" /> Configure Database
        </CardTitle>
        <CardDescription>Enter your PostgreSQL connection URL to get started.</CardDescription>
      </CardHeader>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          setError(null);
          mutation.mutate(url);
        }}
      >
        <CardContent>
          <div className="grid gap-3">
            <Label htmlFor="db-url">Database URL</Label>
            <Input
              id="db-url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="postgres://user:pass@host:5432/dbname"
              required
            />
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
