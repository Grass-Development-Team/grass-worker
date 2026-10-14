export type QuotaPeriod = "none" | "monthly";

export interface QuotaPlanSummary {
  id: string;
  code: string;
  name: string;
  source: "explicit" | "group" | "default";
}

export interface QuotaLimitEntry {
  dimension: string;
  limit: number | null;
  period: QuotaPeriod;
}

export interface QuotaUsageEntry {
  dimension: string;
  limit: number | null;
  used: number;
  period: QuotaPeriod;
}
