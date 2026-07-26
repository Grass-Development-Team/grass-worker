import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlusIcon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { DIMENSION_LABELS } from "@/features/quota/quota.api";

import { adminApi, type AdminQuotaPlan, type QuotaLimitInput } from "../admin.api";

const ALL_DIMENSIONS = Object.keys(DIMENSION_LABELS);

interface LimitDraft {
  unlimited: boolean;
  value: string;
}

type LimitsDraft = Record<string, LimitDraft>;

function draftFromPlan(plan: AdminQuotaPlan | null): LimitsDraft {
  const draft: LimitsDraft = {};
  for (const dimension of ALL_DIMENSIONS) {
    const existing = plan?.limits.find((limit) => limit.dimension === dimension);
    if (existing && existing.limit_value >= 0) {
      draft[dimension] = { unlimited: false, value: String(existing.limit_value) };
    } else {
      draft[dimension] = { unlimited: true, value: "" };
    }
  }
  return draft;
}

// Set values for bounded rows; explicit null removes a row that previously
// existed (edit only) so the dimension returns to unlimited-via-absence.
function limitsPayload(draft: LimitsDraft, plan: AdminQuotaPlan | null): QuotaLimitInput[] {
  const payload: QuotaLimitInput[] = [];
  for (const dimension of ALL_DIMENSIONS) {
    const entry = draft[dimension];
    const hadRow = plan?.limits.some((limit) => limit.dimension === dimension) ?? false;
    if (!entry.unlimited && entry.value.trim() !== "") {
      payload.push({ dimension, limit_value: Number(entry.value) });
    } else if (entry.unlimited && hadRow) {
      payload.push({ dimension, limit_value: null });
    }
  }
  return payload;
}

export function QuotaPlansPanel() {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState<AdminQuotaPlan | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const plansQuery = useQuery({
    queryKey: ["admin", "quota-plans"],
    queryFn: adminApi.listQuotaPlans,
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "quota-plans"] });

  const defaultMutation = useMutation({
    mutationFn: (planId: string) => adminApi.updateQuotaPlan(planId, { is_default: true }),
    onSuccess: () => {
      setError(null);
      invalidate();
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to change the default plan."),
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-4">
        <p className="text-sm text-muted-foreground">
          Plans define per-team limits. Teams resolve their plan via explicit override → team group
          → platform default.
        </p>
        <Button onClick={() => setCreating(true)}>
          <PlusIcon /> New plan
        </Button>
      </div>

      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      {plansQuery.isLoading && <Skeleton className="h-64 w-full" aria-busy="true" />}
      {plansQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          Unable to load quota plans.
        </p>
      )}
      {plansQuery.data && (
        <div className="grid gap-4 lg:grid-cols-2">
          {plansQuery.data.plans.map((plan) => (
            <Card key={plan.id}>
              <CardHeader>
                <div className="flex items-center gap-2">
                  <CardTitle className="text-base">{plan.name}</CardTitle>
                  {plan.is_default && <Badge>Default</Badge>}
                  {!plan.enabled && <Badge variant="destructive">Disabled</Badge>}
                </div>
                <CardDescription>
                  {plan.code}
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
                        <TableCell className="text-sm">
                          {DIMENSION_LABELS[limit.dimension] ?? limit.dimension}
                        </TableCell>
                        <TableCell className="text-right text-sm">
                          {limit.limit_value < 0 ? "Unlimited" : limit.limit_value}
                          {limit.period === "monthly" && (
                            <span className="text-muted-foreground">/mo</span>
                          )}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
              <CardFooter className="gap-2">
                <Button size="sm" variant="outline" onClick={() => setEditing(plan)}>
                  Edit
                </Button>
                {!plan.is_default && plan.enabled && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => defaultMutation.mutate(plan.id)}
                    disabled={defaultMutation.isPending}
                  >
                    Make default
                  </Button>
                )}
              </CardFooter>
            </Card>
          ))}
        </div>
      )}

      {(creating || editing) && (
        <PlanFormDialog
          plan={editing}
          onClose={() => {
            setCreating(false);
            setEditing(null);
          }}
          onSaved={() => {
            setCreating(false);
            setEditing(null);
            invalidate();
          }}
        />
      )}
    </div>
  );
}

function PlanFormDialog({
  plan,
  onClose,
  onSaved,
}: {
  plan: AdminQuotaPlan | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [code, setCode] = useState(plan?.code ?? "");
  const [name, setName] = useState(plan?.name ?? "");
  const [description, setDescription] = useState(plan?.description ?? "");
  const [enabled, setEnabled] = useState(plan?.enabled ?? true);
  const [limits, setLimits] = useState<LimitsDraft>(() => draftFromPlan(plan));

  const mutation = useMutation({
    mutationFn: () =>
      plan
        ? adminApi.updateQuotaPlan(plan.id, {
            name,
            description,
            enabled,
            limits: limitsPayload(limits, plan),
          })
        : adminApi.createQuotaPlan({
            code,
            name,
            description: description || undefined,
            limits: limitsPayload(limits, null),
          }),
    onSuccess: onSaved,
  });

  const setLimit = (dimension: string, update: Partial<LimitDraft>) =>
    setLimits((current) => ({
      ...current,
      [dimension]: { ...current[dimension], ...update },
    }));

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{plan ? `Edit ${plan.name}` : "New quota plan"}</DialogTitle>
          <DialogDescription>
            Unchecked dimensions are unlimited. Lowered limits apply immediately; usage already
            above a new limit is kept but blocks further consumption.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim() && (plan || code.trim())) mutation.mutate();
          }}
        >
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="plan-code">Code</FieldLabel>
              <Input
                id="plan-code"
                value={code}
                onChange={(event) => setCode(event.target.value)}
                disabled={plan != null}
                placeholder="team-plus"
                required={plan == null}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="plan-name">Name</FieldLabel>
              <Input
                id="plan-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                required
              />
            </Field>
          </div>
          <Field>
            <FieldLabel htmlFor="plan-description">Description</FieldLabel>
            <Input
              id="plan-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
          </Field>
          {plan && (
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                className="size-4"
                checked={enabled}
                disabled={plan.is_default}
                onChange={(event) => setEnabled(event.target.checked)}
              />
              Enabled
              {plan.is_default && (
                <span className="text-xs text-muted-foreground">
                  (the default plan must stay enabled)
                </span>
              )}
            </label>
          )}

          <div className="space-y-2">
            <p className="text-sm font-medium">Limits</p>
            {ALL_DIMENSIONS.map((dimension) => {
              const entry = limits[dimension];
              return (
                <div key={dimension} className="flex items-center gap-2">
                  <span className="w-56 shrink-0 text-sm">{DIMENSION_LABELS[dimension]}</span>
                  <Input
                    type="number"
                    min={0}
                    className="h-8"
                    aria-label={`${DIMENSION_LABELS[dimension]} limit`}
                    value={entry.unlimited ? "" : entry.value}
                    disabled={entry.unlimited}
                    onChange={(event) => setLimit(dimension, { value: event.target.value })}
                  />
                  <label className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
                    <input
                      type="checkbox"
                      className="size-3.5"
                      checked={entry.unlimited}
                      onChange={(event) => setLimit(dimension, { unlimited: event.target.checked })}
                    />
                    Unlimited
                  </label>
                </div>
              );
            })}
          </div>

          {mutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {mutation.error instanceof Error ? mutation.error.message : "Unable to save."}
            </p>
          )}
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Saving…" : plan ? "Save plan" : "Create plan"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
