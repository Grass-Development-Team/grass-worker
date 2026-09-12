export type StorageBackend = "local" | "s3" | "minio" | "r2";

export interface StorageConfiguration {
  backend: StorageBackend;
  local_root: string;
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  force_path_style: boolean;
  allow_http: boolean;
  credentials_configured: boolean;
}

export interface StorageInput {
  backend: StorageBackend;
  local_root?: string;
  endpoint?: string;
  region?: string;
  bucket?: string;
  prefix?: string;
  force_path_style?: boolean;
  allow_http?: boolean;
  access_key_id?: string;
  secret_access_key?: string;
  session_token?: string;
}

interface StorageFields {
  backend: StorageBackend;
  localRoot: string;
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  forcePathStyle: boolean;
  allowHttp: boolean;
  accessKeyId: string;
  secretAccessKey: string;
  sessionToken: string;
}

export function storageDefaults(backend: StorageBackend) {
  return {
    region: backend === "r2" ? "auto" : "us-east-1",
    forcePathStyle: backend === "minio",
    allowHttp: backend === "minio",
  };
}

export function storageInput(fields: StorageFields): StorageInput {
  const { backend, localRoot } = fields;
  if (backend === "local") return { backend, local_root: localRoot.trim() };
  return {
    backend,
    local_root: localRoot.trim(),
    endpoint: fields.endpoint.trim(),
    region: fields.region.trim(),
    bucket: fields.bucket.trim(),
    prefix: fields.prefix.trim().replace(/^\/+|\/+$/g, ""),
    force_path_style: fields.forcePathStyle,
    allow_http: fields.allowHttp,
    ...(fields.accessKeyId.trim() && { access_key_id: fields.accessKeyId.trim() }),
    ...(fields.secretAccessKey.trim() && { secret_access_key: fields.secretAccessKey.trim() }),
    ...(fields.sessionToken.trim() && { session_token: fields.sessionToken.trim() }),
  };
}
