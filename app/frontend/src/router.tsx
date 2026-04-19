import { Navigate, type RouteObject } from "react-router-dom";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { LoginPage } from "./routes/login-page";
import { ProtectedRoute } from "./routes/protected-route";
import { ProjectsPage } from "./routes/projects-page";
import { SetupPage } from "./routes/setup-page";
import { SystemModeGate } from "./routes/system-mode-gate";

function ProtectedNotFound() {
  return (
    <main className="flex min-h-screen items-center justify-center bg-muted/30 p-6">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>Page not found</CardTitle>
          <CardDescription>
            This protected route exists behind authentication, but there is no
            page registered for it yet.
          </CardDescription>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground">
          Continue from the console home or add the target route next.
        </CardContent>
      </Card>
    </main>
  );
}

export const routes: RouteObject[] = [
  {
    element: <SystemModeGate />,
    children: [
      {
        path: "/setup",
        element: <SetupPage />,
      },
      {
        path: "/login",
        element: <LoginPage />,
      },
      {
        element: <ProtectedRoute />,
        children: [
          {
            index: true,
            element: <Navigate replace to="/projects" />,
          },
          {
            path: "/projects",
            element: <ProjectsPage />,
          },
          {
            path: "*",
            element: <ProtectedNotFound />,
          },
        ],
      },
    ],
  },
];
