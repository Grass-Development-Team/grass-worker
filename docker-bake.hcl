variable "IMAGE" {
  default = "ghcr.io/grass-development-team/grass-worker"
}

variable "TAGS_DEBIAN" {
  default = ""
}

variable "TAGS_SLIM" {
  default = ""
}

variable "TAGS_ALPINE" {
  default = ""
}

variable "LABELS_JSON" {
  default = "{}"
}

group "default" {
  targets = ["debian", "slim", "alpine"]
}

target "_common" {
  context = "."
  dockerfile = "Dockerfile"
  cache-from = ["type=gha,scope=grass-worker"]
  cache-to = ["type=gha,scope=grass-worker,mode=max"]
  labels = jsondecode(LABELS_JSON)
}

target "debian" {
  inherits = ["_common"]
  target = "runtime"
  tags = split("\n", TAGS_DEBIAN)
}

target "slim" {
  inherits = ["_common"]
  target = "runtime-slim"
  tags = split("\n", TAGS_SLIM)
}

target "alpine" {
  inherits = ["_common"]
  target = "runtime-alpine"
  tags = split("\n", TAGS_ALPINE)
}
