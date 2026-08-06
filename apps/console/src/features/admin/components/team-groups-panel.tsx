import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlusIcon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { adminApi, type AdminTeamGroup } from "../admin.api";

const INHERIT_NONE = "__none__";
const INHERIT_REVIEW = "inherit";

const reviewModeLabel = (mode: "auto" | "manual" | null) =>
  mode ? `${mode.charAt(0).toUpperCase()}${mode.slice(1)}` : "Inherit";

const reviewPolicyLabel = (group: AdminTeamGroup) =>
  `Production ${reviewModeLabel(group.review_policy.production)} · Preview ${reviewModeLabel(group.review_policy.preview)} · Domain ${reviewModeLabel(group.review_policy.domain)}`;

export function TeamGroupsPanel() {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState<AdminTeamGroup | null>(null);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<AdminTeamGroup | null>(null);

  const groupsQuery = useQuery({
    queryKey: ["admin", "team-groups"],
    queryFn: adminApi.listTeamGroups,
  });
  const plansQuery = useQuery({
    queryKey: ["admin", "quota-plans"],
    queryFn: adminApi.listQuotaPlans,
  });

  const planName = (planId: string | null) =>
    plansQuery.data?.plans.find((plan) => plan.id === planId)?.name ?? (planId ? planId : "—");

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "team-groups"] });

  const defaultMutation = useMutation({
    mutationFn: (groupId: string) => adminApi.updateTeamGroup(groupId, { is_default: true }),
    onSuccess: invalidate,
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-4">
        <p className="text-sm text-muted-foreground">
          Groups map teams to quota plans and platform-managed review policy overrides.
        </p>
        <Button onClick={() => setCreating(true)}>
          <PlusIcon /> New group
        </Button>
      </div>

      {groupsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {groupsQuery.data && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Group</TableHead>
              <TableHead>Quota plan</TableHead>
              <TableHead>Review policy</TableHead>
              <TableHead>Teams</TableHead>
              <TableHead className="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {groupsQuery.data.groups.map((group) => (
              <TableRow key={group.id}>
                <TableCell>
                  <span className="flex items-center gap-2 font-medium">
                    {group.name}
                    {group.is_default && <Badge>Default</Badge>}
                  </span>
                  <p className="text-xs text-muted-foreground">
                    {group.code}
                    {group.description ? ` — ${group.description}` : ""}
                  </p>
                </TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {planName(group.quota_plan_id)}
                </TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {reviewPolicyLabel(group)}
                </TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {group.team_count ?? 0}
                </TableCell>
                <TableCell className="text-right">
                  <div className="flex justify-end gap-1">
                    <Button size="sm" variant="outline" onClick={() => setEditing(group)}>
                      Edit
                    </Button>
                    {!group.is_default && (
                      <>
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => defaultMutation.mutate(group.id)}
                          disabled={defaultMutation.isPending}
                        >
                          Make default
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="text-destructive"
                          onClick={() => setDeleting(group)}
                        >
                          Delete
                        </Button>
                      </>
                    )}
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      {(creating || editing) && (
        <GroupFormDialog
          group={editing}
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
      {deleting && (
        <DeleteGroupDialog
          group={deleting}
          onClose={() => setDeleting(null)}
          onDeleted={() => {
            setDeleting(null);
            invalidate();
          }}
        />
      )}
    </div>
  );
}

function GroupFormDialog({
  group,
  onClose,
  onSaved,
}: {
  group: AdminTeamGroup | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [code, setCode] = useState(group?.code ?? "");
  const [name, setName] = useState(group?.name ?? "");
  const [description, setDescription] = useState(group?.description ?? "");
  const [planId, setPlanId] = useState(group?.quota_plan_id ?? INHERIT_NONE);
  const [reviewProduction, setReviewProduction] = useState(
    group?.review_policy.production ?? INHERIT_REVIEW,
  );
  const [reviewPreview, setReviewPreview] = useState(
    group?.review_policy.preview ?? INHERIT_REVIEW,
  );
  const [reviewDomain, setReviewDomain] = useState(group?.review_policy.domain ?? INHERIT_REVIEW);

  const plansQuery = useQuery({
    queryKey: ["admin", "quota-plans"],
    queryFn: adminApi.listQuotaPlans,
  });

  const mutation = useMutation({
    mutationFn: () =>
      group
        ? adminApi.updateTeamGroup(group.id, {
            name,
            description,
            quota_plan_id: planId === INHERIT_NONE ? null : planId,
            review_policy: {
              production: reviewProduction === INHERIT_REVIEW ? null : reviewProduction,
              preview: reviewPreview === INHERIT_REVIEW ? null : reviewPreview,
              domain: reviewDomain === INHERIT_REVIEW ? null : reviewDomain,
            },
          })
        : adminApi.createTeamGroup({
            code,
            name,
            description: description || undefined,
            quota_plan_id: planId === INHERIT_NONE ? undefined : planId,
            review_policy: {
              production: reviewProduction === INHERIT_REVIEW ? null : reviewProduction,
              preview: reviewPreview === INHERIT_REVIEW ? null : reviewPreview,
              domain: reviewDomain === INHERIT_REVIEW ? null : reviewDomain,
            },
          }),
    onSuccess: onSaved,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{group ? `Edit ${group.name}` : "New team group"}</DialogTitle>
          <DialogDescription>
            Teams in a group without a plan fall back to the platform default plan.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (name.trim() && (group || code.trim())) mutation.mutate();
          }}
        >
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="group-code">Code</FieldLabel>
              <Input
                id="group-code"
                value={code}
                onChange={(event) => setCode(event.target.value)}
                disabled={group != null}
                placeholder="enterprise"
                required={group == null}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="group-name">Name</FieldLabel>
              <Input
                id="group-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                required
              />
            </Field>
          </div>
          <Field>
            <FieldLabel htmlFor="group-description">Description</FieldLabel>
            <Input
              id="group-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
          </Field>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="group-review-production">Production review</FieldLabel>
              <Select value={reviewProduction} onValueChange={setReviewProduction}>
                <SelectTrigger id="group-review-production" aria-label="Production review">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={INHERIT_REVIEW}>Inherit</SelectItem>
                  <SelectItem value="auto">Auto</SelectItem>
                  <SelectItem value="manual">Manual</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel htmlFor="group-review-preview">Preview review</FieldLabel>
              <Select value={reviewPreview} onValueChange={setReviewPreview}>
                <SelectTrigger id="group-review-preview" aria-label="Preview review">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={INHERIT_REVIEW}>Inherit</SelectItem>
                  <SelectItem value="auto">Auto</SelectItem>
                  <SelectItem value="manual">Manual</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel htmlFor="group-review-domain">Domain review</FieldLabel>
              <Select value={reviewDomain} onValueChange={setReviewDomain}>
                <SelectTrigger id="group-review-domain" aria-label="Domain review">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={INHERIT_REVIEW}>Inherit</SelectItem>
                  <SelectItem value="auto">Auto</SelectItem>
                  <SelectItem value="manual">Manual</SelectItem>
                </SelectContent>
              </Select>
            </Field>
          </div>
          <Field>
            <FieldLabel htmlFor="group-plan">Quota plan</FieldLabel>
            <Select value={planId} onValueChange={setPlanId}>
              <SelectTrigger id="group-plan">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={INHERIT_NONE}>None — use platform default</SelectItem>
                {plansQuery.data?.plans
                  .filter((plan) => plan.enabled)
                  .map((plan) => (
                    <SelectItem key={plan.id} value={plan.id}>
                      {plan.name} ({plan.code})
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Saving…" : group ? "Save group" : "Create group"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function DeleteGroupDialog({
  group,
  onClose,
  onDeleted,
}: {
  group: AdminTeamGroup;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const mutation = useMutation({
    mutationFn: () => adminApi.deleteTeamGroup(group.id),
    onSuccess: onDeleted,
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Delete team group</DialogTitle>
          <DialogDescription>
            Deletes “{group.name}”. Groups with assigned teams are refused.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => mutation.mutate()}
            disabled={mutation.isPending}
          >
            {mutation.isPending ? "Deleting…" : "Delete group"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
