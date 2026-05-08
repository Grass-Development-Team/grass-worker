import * as React from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

type CreateDeploymentCardProps = {
  disabled: boolean;
  disabledReason: string | null;
  error: string | null;
  isCreating: boolean;
  onCreateDeployment: (input: {
    source_branch?: string;
    source_revision?: string;
  }) => void;
  onResetError: () => void;
  resetToken: number;
};

type ProductionDeploymentCardProps = {
  disabled: boolean;
  disabledReason: string | null;
  error: string | null;
  isCreating: boolean;
  productionBranch: string;
  repositoryUrl: string;
  onCreateDeployment: () => void;
  onResetError: () => void;
};

function normalizeOptionalInput(value: string) {
  const normalized = value.trim();

  return normalized === "" ? undefined : normalized;
}

export function CreateDeploymentCard({
  disabled,
  disabledReason,
  error,
  isCreating,
  onCreateDeployment,
  onResetError,
  resetToken,
}: CreateDeploymentCardProps) {
  const [sourceBranch, setSourceBranch] = React.useState("");
  const [sourceRevision, setSourceRevision] = React.useState("");

  React.useEffect(() => {
    setSourceBranch("");
    setSourceRevision("");
  }, [resetToken]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Create deployment</h2>
        </CardTitle>
        <CardDescription>
          Record a new deployment intent for this project before execution automation lands.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            onCreateDeployment({
              source_branch: normalizeOptionalInput(sourceBranch),
              source_revision: normalizeOptionalInput(sourceRevision),
            });
          }}
        >
          <div className="space-y-2">
            <Label htmlFor="deployment-source-branch">Source branch</Label>
            <Input
              id="deployment-source-branch"
              disabled={disabled || isCreating}
              onChange={(event) => {
                setSourceBranch(event.target.value);
                onResetError();
              }}
              placeholder="main"
              value={sourceBranch}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="deployment-source-revision">Source revision</Label>
            <Input
              id="deployment-source-revision"
              disabled={disabled || isCreating}
              onChange={(event) => {
                setSourceRevision(event.target.value);
                onResetError();
              }}
              placeholder="deadbeef"
              value={sourceRevision}
            />
          </div>
          {disabledReason ? (
            <Alert>
              <AlertTitle>Deployment creation unavailable</AlertTitle>
              <AlertDescription>{disabledReason}</AlertDescription>
            </Alert>
          ) : null}
          {error ? (
            <Alert variant="destructive">
              <AlertTitle>Deployment creation failed</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
          <Button className="w-full" disabled={disabled || isCreating} type="submit">
            {isCreating ? "Creating deployment..." : "Create deployment"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}

export function ProductionDeploymentCard({
  disabled,
  disabledReason,
  error,
  isCreating,
  productionBranch,
  repositoryUrl,
  onCreateDeployment,
  onResetError,
}: ProductionDeploymentCardProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Deploy production branch</h2>
        </CardTitle>
        <CardDescription>
          Queue a node-worker build from the configured production branch.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid gap-4 text-sm text-muted-foreground">
          <div className="space-y-1">
            <p>Repository</p>
            <p className="break-all font-medium text-foreground">{repositoryUrl}</p>
          </div>
          <div className="space-y-1">
            <p>Production branch</p>
            <p className="font-medium text-foreground">{productionBranch}</p>
          </div>
        </div>
        {disabledReason ? (
          <Alert>
            <AlertTitle>Deployment creation unavailable</AlertTitle>
            <AlertDescription>{disabledReason}</AlertDescription>
          </Alert>
        ) : null}
        {error ? (
          <Alert variant="destructive">
            <AlertTitle>Deployment creation failed</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}
        <Button
          className="w-full"
          disabled={disabled || isCreating}
          onClick={() => {
            onResetError();
            onCreateDeployment();
          }}
          type="button"
        >
          {isCreating ? "Queuing production deploy..." : "Deploy production branch"}
        </Button>
      </CardContent>
    </Card>
  );
}
