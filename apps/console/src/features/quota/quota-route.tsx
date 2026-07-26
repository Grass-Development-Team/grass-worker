import { useQuery } from "@tanstack/react-query";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useTeam } from "@/features/teams/team-context";

import { dimensionLabel, quotaApi } from "./quota.api";

function formatLimit(limit: number | null): string {
  return limit === null ? "Unlimited" : limit.toLocaleString();
}

function usagePercent(used: number, limit: number | null): number | null {
  if (limit === null || limit <= 0) return null;
  return Math.min(100, Math.round((used / limit) * 100));
}

export function QuotaRoute() {
  const { activeTeam } = useTeam();
  const teamId = activeTeam?.id;

  const usageQuery = useQuery({
    queryKey: ["quota-usage", teamId],
    queryFn: () => quotaApi.usage(teamId as string),
    enabled: Boolean(teamId),
  });

  if (!teamId) {
    return <p className="text-sm text-muted-foreground">Select a team to view quota usage.</p>;
  }

  if (usageQuery.isLoading) {
    return (
      <div className="space-y-4" aria-busy="true">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  if (usageQuery.isError) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {usageQuery.error instanceof Error ? usageQuery.error.message : "Unable to load quota."}
      </p>
    );
  }

  const data = usageQuery.data;
  if (!data) return null;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-lg font-semibold">Usage &amp; quota</h1>
        <p className="text-sm text-muted-foreground">
          Current usage for {activeTeam?.name} against its quota plan.
        </p>
      </div>
      <Card>
        <CardHeader>
          <CardTitle>{data.plan.name}</CardTitle>
          <CardDescription>
            Plan <code>{data.plan.code}</code> applied via {data.plan.source}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Dimension</TableHead>
                <TableHead className="text-right">Used</TableHead>
                <TableHead className="text-right">Limit</TableHead>
                <TableHead className="w-48">Usage</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.usage.map((entry) => {
                const percent = usagePercent(entry.used, entry.limit);
                const nearLimit = percent !== null && percent >= 80;
                return (
                  <TableRow key={entry.dimension}>
                    <TableCell>
                      <span className="font-medium">{dimensionLabel(entry.dimension)}</span>
                      {entry.period === "monthly" && (
                        <span className="ml-2 text-xs text-muted-foreground">monthly</span>
                      )}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {entry.used.toLocaleString()}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {formatLimit(entry.limit)}
                    </TableCell>
                    <TableCell>
                      {percent === null ? (
                        <span className="text-xs text-muted-foreground">—</span>
                      ) : (
                        <div className="flex items-center gap-2">
                          <div
                            className="h-2 flex-1 overflow-hidden rounded-full bg-muted"
                            role="progressbar"
                            aria-valuenow={percent}
                            aria-valuemin={0}
                            aria-valuemax={100}
                            aria-label={`${dimensionLabel(entry.dimension)} usage`}
                          >
                            <div
                              className={nearLimit ? "h-full bg-destructive" : "h-full bg-primary"}
                              style={{ width: `${percent}%` }}
                            />
                          </div>
                          <span className="w-10 text-right text-xs tabular-nums text-muted-foreground">
                            {percent}%
                          </span>
                        </div>
                      )}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  );
}
