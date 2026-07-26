import { useQuery } from "@tanstack/react-query";

import { Badge } from "@/components/ui/badge";
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
import { dimensionLabel } from "@/features/quota/quota.api";

import { adminApi } from "../admin.api";

export function QuotaPlansPanel() {
  const plansQuery = useQuery({
    queryKey: ["admin", "quota-plans"],
    queryFn: adminApi.listQuotaPlans,
  });

  if (plansQuery.isLoading) return <Skeleton className="h-64 w-full" aria-busy="true" />;
  if (plansQuery.isError) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {plansQuery.error instanceof Error
          ? plansQuery.error.message
          : "Unable to load quota plans."}
      </p>
    );
  }

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      {plansQuery.data?.plans.map((plan) => (
        <Card key={plan.id}>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              {plan.name}
              {plan.is_default && <Badge variant="outline">Default</Badge>}
              {!plan.enabled && <Badge variant="secondary">Disabled</Badge>}
            </CardTitle>
            <CardDescription>
              <code>{plan.code}</code>
              {plan.description ? ` — ${plan.description}` : ""}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Dimension</TableHead>
                  <TableHead className="text-right">Limit</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {plan.limits.map((limit) => (
                  <TableRow key={limit.dimension}>
                    <TableCell>{dimensionLabel(limit.dimension)}</TableCell>
                    <TableCell className="text-right tabular-nums">
                      {limit.limit_value < 0 ? "Unlimited" : limit.limit_value.toLocaleString()}
                      {limit.period === "monthly" && (
                        <span className="ml-1 text-xs text-muted-foreground">/mo</span>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
