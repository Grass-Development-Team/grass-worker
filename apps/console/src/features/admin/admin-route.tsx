import { ShieldCheckIcon } from "lucide-react";

export function AdminRoute() {
  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold">Administration</h1>
        <p className="text-sm text-muted-foreground">System-level configuration and operations.</p>
      </div>
      <div className="flex items-center gap-4 border-y py-6">
        <div className="flex size-10 shrink-0 items-center justify-center rounded-md bg-muted">
          <ShieldCheckIcon className="size-5" />
        </div>
        <div>
          <p className="font-medium">Grass Worker Control API</p>
          <p className="text-sm text-muted-foreground">Ready mode | Version 0.1.0</p>
        </div>
      </div>
    </div>
  );
}
