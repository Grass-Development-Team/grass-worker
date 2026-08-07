import { useMutation } from "@tanstack/react-query";
import { ArrowRight, Check } from "lucide-react";

import { setupApi } from "@/features/setup/setup.api";
import { useBranding } from "@/features/branding/branding-context";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

export function FinishStep({ onSuccess }: { onSuccess: () => void }) {
  const { siteName } = useBranding();
  const mutation = useMutation({
    mutationFn: setupApi.finishSetup,
    onSuccess: () => {
      onSuccess();
    },
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Check className="size-5" /> Complete Setup
        </CardTitle>
        <CardDescription>
          All steps are done. Click below to finish setup and start using {siteName}.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {mutation.isSuccess && (
          <div className="flex items-center gap-2 text-sm text-green-600 dark:text-green-400 rounded-lg border border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-950 p-4">
            <Check className="size-4" /> Setup complete! Redirecting to login...
          </div>
        )}
      </CardContent>
      <CardFooter>
        <Button
          className="w-full"
          onClick={() => mutation.mutate()}
          disabled={mutation.isPending || mutation.isSuccess}
        >
          {mutation.isPending
            ? "Finishing..."
            : mutation.isSuccess
              ? "Setup Complete!"
              : "Finish Setup"}
          {!mutation.isPending && !mutation.isSuccess && <ArrowRight className="ml-2 size-4" />}
        </Button>
      </CardFooter>
    </Card>
  );
}
