import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";

interface SettingsCardProps {
  title: React.ReactNode;
  description?: React.ReactNode;
  /** Muted helper text shown in the footer, left of the action. */
  hint?: React.ReactNode;
  /** Footer action, usually a submit button for the surrounding form. */
  action?: React.ReactNode;
  variant?: "default" | "destructive";
  contentClassName?: string;
  children?: React.ReactNode;
}

export function SettingsCard({
  title,
  description,
  hint,
  action,
  variant = "default",
  contentClassName,
  children,
}: SettingsCardProps) {
  const hasFooter = Boolean(hint || action);

  return (
    <Card
      className={cn(
        "gap-0 overflow-hidden py-0",
        variant === "destructive" && "border-destructive/40",
      )}
    >
      <CardHeader className="gap-1.5 px-6 pt-6">
        <CardTitle className="text-base">{title}</CardTitle>
        {description && <CardDescription>{description}</CardDescription>}
      </CardHeader>
      <CardContent className={cn("px-6 py-5", contentClassName)}>{children}</CardContent>
      {hasFooter && (
        <div
          className={cn(
            "flex min-h-13 items-center justify-between gap-4 border-t px-6 py-3",
            variant === "destructive" ? "border-destructive/40 bg-destructive/5" : "bg-muted/40",
          )}
        >
          <div className="text-[0.8125rem] text-muted-foreground">{hint}</div>
          {action && <div className="flex shrink-0 items-center gap-2">{action}</div>}
        </div>
      )}
    </Card>
  );
}
