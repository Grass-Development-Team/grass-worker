import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { AlertCircle, ArrowRight, Check, Copy, Server } from "lucide-react";

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

export function NodeStep({
  onCreated,
  onContinue,
  token,
}: {
  onCreated: (token: string) => void;
  onContinue: () => void;
  token: string | null;
}) {
  const [name, setName] = useState("local-node");
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState<string | null>(null);
  const mutation = useMutation({
    mutationFn: () => setupApi.createNode(name || undefined),
    onSuccess: (data) => {
      onCreated(data.token);
    },
    onError: (err: Error) => setError(err.message),
  });

  if (token) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-green-600 dark:text-green-400">
            <Check className="size-5" /> Node Created
          </CardTitle>
          <CardDescription>Save this token — it won&apos;t be shown again.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3">
            <div className="rounded-lg border bg-muted p-4">
              <p className="text-xs font-medium text-muted-foreground mb-1">NODE TOKEN</p>
              <div className="flex items-center gap-2">
                <code className="min-w-0 flex-1 break-all font-mono text-sm">{token}</code>
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  aria-label={copied ? "Node token copied" : "Copy node token"}
                  onClick={async () => {
                    try {
                      await navigator.clipboard.writeText(token);
                      setCopied(true);
                      setCopyError(null);
                    } catch {
                      setCopyError("Unable to copy the node token.");
                    }
                  }}
                >
                  {copied ? <Check /> : <Copy />}
                </Button>
              </div>
            </div>
            {copyError && (
              <p role="alert" className="text-sm text-destructive">
                {copyError}
              </p>
            )}
            <p className="text-sm text-muted-foreground">
              Copy this token and set it as <code>node_token</code> in your node configuration.
            </p>
          </div>
        </CardContent>
        <CardFooter>
          <Button className="w-full" variant="outline" onClick={onContinue}>
            Continue <ArrowRight className="ml-2 size-4" />
          </Button>
        </CardFooter>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Server className="size-5" /> Create First Node
        </CardTitle>
        <CardDescription>A node runs builds and serves your deployed sites.</CardDescription>
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
            <Label htmlFor="node-name">Node Name</Label>
            <Input
              id="node-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="local-node"
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
            {mutation.isPending ? "Creating..." : "Create Node"}
            {!mutation.isPending && <ArrowRight className="ml-2 size-4" />}
          </Button>
        </CardFooter>
      </form>
    </Card>
  );
}
