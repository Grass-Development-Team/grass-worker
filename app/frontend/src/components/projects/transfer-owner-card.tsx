import * as React from "react";
import type { Project } from "@/api/projects";
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

type TransferOwnerCardProps = {
  error: string | null;
  isTransferring: boolean;
  onResetError: () => void;
  onTransfer: (ownerEmail: string) => void;
  project: Project;
};

export function TransferOwnerCard({
  error,
  isTransferring,
  onResetError,
  onTransfer,
  project,
}: TransferOwnerCardProps) {
  const [ownerEmail, setOwnerEmail] = React.useState("");
  const [validationError, setValidationError] = React.useState<string | null>(null);
  const disabled = project.status === "soft_deleted";
  const transferError = validationError ?? error;

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Transfer owner</h2>
        </CardTitle>
        <CardDescription>
          Move this project to another existing account by email address.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();

            const email = ownerEmail.trim();
            if (!email) {
              setValidationError("Owner email is required");
              return;
            }

            setValidationError(null);
            onTransfer(email);
          }}
        >
          <div className="space-y-2">
            <Label htmlFor="transfer-owner-email">New owner email</Label>
            <Input
              disabled={disabled}
              id="transfer-owner-email"
              onChange={(event) => {
                setOwnerEmail(event.target.value);
                setValidationError(null);
                onResetError();
              }}
              placeholder="new-owner@example.com"
              type="email"
              value={ownerEmail}
            />
          </div>
          {transferError ? (
            <Alert variant="destructive">
              <AlertTitle>Transfer failed</AlertTitle>
              <AlertDescription>{transferError}</AlertDescription>
            </Alert>
          ) : null}
          <Button disabled={disabled || isTransferring} type="submit" variant="outline">
            {isTransferring ? "Transferring owner..." : "Transfer owner"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
