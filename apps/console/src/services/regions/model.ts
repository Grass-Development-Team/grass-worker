export interface Region {
  code: string;
  name: string;
  ingress_hostname?: string | null;
  ingress_enabled?: boolean;
}
