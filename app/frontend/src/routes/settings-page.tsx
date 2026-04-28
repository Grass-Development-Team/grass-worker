import { ConsolePageHeader } from "@/components/console/console-page-header";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

export function SettingsPage() {
  return (
    <div className="space-y-6">
      <ConsolePageHeader
        description="User settings for signed-in console accounts will land here first."
        eyebrow="Account"
        title="Settings"
      />

      <Card>
        <CardHeader>
          <CardTitle>Settings are not implemented yet</CardTitle>
          <CardDescription>
            This route exists so the shared console navigation can expose a stable user settings
            entry before the settings feature is built.
          </CardDescription>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground">
          Keep future user-scoped preferences here instead of mixing them into project or admin
          pages.
        </CardContent>
      </Card>
    </div>
  );
}
