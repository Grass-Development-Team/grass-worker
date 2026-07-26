import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { AlertCircle, ArrowRight, Package } from "lucide-react";

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

export function StorageStep({ onSuccess }: { onSuccess: () => void }) {
  const [root, setRoot] = useState("/data");
  const [error, setError] = useState<string | null>(null);
  const mutation = useMutation({
    mutationFn: () => setupApi.configureStorage(root || undefined),
    onSuccess,
    onError: (err: Error) => setError(err.message),
  });
  const skipMutation = useMutation({
    mutationFn: () => setupApi.configureStorage("/data"),
    onSuccess,
    onError: (err: Error) => setError(err.message),
  });
  const isPending = mutation.isPending || skipMutation.isPending;
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Package className="size-5" /> Configure Storage
        </CardTitle>
        <CardDescription>
          Set the root directory for artifact and deployment storage. You can skip this and use the
          default later.
        </CardDescription>
      </CardHeader>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          setError(null);
          mutation.mutate();
        }}
      >
        <CardContent>
          <div className="grid gap-3">
            <Label htmlFor="storage-root">Storage Root</Label>
            <Input
              id="storage-root"
              value={root}
              onChange={(e) => setRoot(e.target.value)}
              placeholder="/data"
            />
            {error && (
              <div role="alert" className="flex items-center gap-2 text-sm text-destructive">
                <AlertCircle className="size-4" />
                {error}
              </div>
            )}
          </div>
        </CardContent>
        <CardFooter className="flex flex-col gap-2">
          <Button type="submit" className="w-full" disabled={isPending}>
            {mutation.isPending ? "Saving..." : "Save Storage Path"}
            {!mutation.isPending && <ArrowRight className="ml-2 size-4" />}
          </Button>
          <Button
            type="button"
            variant="ghost"
            className="w-full"
            disabled={isPending}
            onClick={() => {
              setError(null);
              skipMutation.mutate();
            }}
          >
            {skipMutation.isPending ? "Skipping..." : "Skip for now (use /data)"}
          </Button>
        </CardFooter>
      </form>
    </Card>
  );
}
