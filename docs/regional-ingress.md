# Regional ingress and HTTPS

A regional ingress gives custom domains one stable CNAME target. Each entry
Node uses the client's original Host to look up the deployment in its full
route snapshot. A request reaches its assigned Serve Node through at most one
Peer Hop, including when that deployment lives in another region.

## Configure the entry Nodes

Use at least two Serve Nodes in the same region. Configure a distinct Node
identity and private `public_base_url` on each; the values below are examples.

```toml
[node]
id = "eu-entry-a"
region = "eu"

[node.capabilities]
build = false
serve = true

[serve]
host = "0.0.0.0"
port = 8080
public_base_url = "http://eu-entry-a.internal:8080"
artifact_cache_root = "/data/node/artifacts"

[serve.tls]
enabled = true
port = 8443
```

Merge these settings into the Node configuration containing its Control API
URL and Node token. TLS defaults to disabled on port `8443` for compatibility
with existing configurations. A configured TLS port must be nonzero and
different from the HTTP port. The Console's Node configuration editor also
exposes native HTTPS and its port; restart the Node after applying desired
configuration and confirm the effective revision and TLS readiness.

The HTTP listener remains available for HTTP-01 and internal communication.
Keep its address reachable from the Control API, the regional load balancer,
and every Serve Node that may forward traffic. Use private networking or a
Tailscale/WireGuard network for these addresses. A Node endpoint URL is not a
site URL: native HTTPS rejects requests whose TLS SNI differs from the site
Host, so use private HTTP for `serve.public_base_url` when enabling native TLS.

### Gateway authentication and mixed Nodes

Gateway token authentication is the default. A Serve Node registered with the
Control API receives the outbound credential through its authenticated Node
session; it is not placed in desired configuration. On a Node whose Serve
listeners are reachable only by trusted peers, the inbound mode can be changed:

```toml
[security]
gateway_authentication = "none"
```

The sender follows the target Node's advertised mode. A target using `token`
receives the gateway token even if the sending Node accepts `none`; a target
using `none` receives the single-hop marker without a token. All four source
and target mode combinations therefore work. Missing credentials for a token
target produce an error rather than falling back to unauthenticated traffic.

Every mode retains Host binding and the one-hop limit. The Control API's Node
endpoints continue to require the individual Node bearer token. Use token
mode for public entry Nodes, including TLS entry listeners: TCP passthrough
cannot filter an encrypted request for an internal gateway path. Token
authentication does not encrypt private HTTP traffic; use an encrypted mesh
when the transport network is not trusted.

## Configure the regional load balancer

[deploy/haproxy-regional.cfg](../deploy/haproxy-regional.cfg) is a HAProxy
2.8+ example with HTTP on port 80 and TCP passthrough on port 443. Replace its
documentation IP addresses and the regional hostname with your deployment
values. Keep TLS termination on the Nodes so every custom hostname receives
its own matching certificate; the CNAME target's certificate does not cover
the custom domain.

```sh
haproxy -c -f deploy/haproxy-regional.cfg
haproxy -W -db -f deploy/haproxy-regional.cfg
```

The example preserves the incoming Host and TLS SNI. It does not use PROXY
protocol, which the Node listener does not accept; Nodes therefore see the
load balancer's source IP. Do not add `send-proxy` to the backend servers.
Firewall backend ports so only the load balancer and trusted internal clients
can reach them. Public entry Nodes should keep token gateway authentication.

Both backends check `GET /_grass/health` over each Node's HTTP port. This
reserved endpoint works without a deployment binding and reports readiness
after the route snapshot and configured listeners are available. Use the same
path in **Administration → Regional ingresses**. Certificate installation and
Node TLS readiness are shown separately in the ingress status.

Point the ingress hostname's A/AAAA records at the load balancer. A DNS record
listing several Node addresses is not a substitute for health-based removal:
clients may keep a failed address until its cached DNS answer expires. The
example removes an unhealthy entry after two failed checks and admits it
again after two successful checks. Existing connections to an entry that
fails must reconnect; new requests use an available entry.

## Bind and verify a custom hostname

1. Under **Administration → Regional ingresses**, add the Node region and an
   ingress hostname such as `eu.entry.example.net`. Enable the ingress and TLS,
   select the certificate issuer, and point its public DNS records at the entries.
2. Add the custom domain in the project's Domains page and select that region.
   Publish the displayed CNAME target and the exact `_grass.<domain>` TXT
   ownership record. The TXT value is bound to this domain binding; copy the
   Console value rather than deriving or reusing one from another binding.
3. Run ownership verification. The binding must also pass the configured
   automatic or manual domain review policy before it serves traffic or
   becomes eligible for certificate issuance.
4. Keep public TCP port 80 reachable through every entry for HTTP-01. The Node
   serves only the published host/token challenge while it is unexpired,
   even before a deployment is active. Ordinary requests still require a
   Host binding and an active deployment.
5. Wait for the certificate to become active and the entry Nodes to report its
   installed revision, then open the custom HTTPS URL.

The Control API waits for healthy regional entries to acknowledge the current
HTTP challenge snapshot before it asks the CA to validate the challenge. Each
Node polls snapshots every five seconds. An unreachable Control API preserves
the last valid local certificate; explicit Node authorization revocation or
an authoritative snapshot removing the hostname withdraws it.

Automatic certificate issuance uses HTTP-01 for both regional ingress and
custom hostnames. Keep public port 80 reachable for issuance and renewal.
When that is unavailable, import and renew a manual certificate. Custom
domains still require the independent `_grass` TXT ownership verification.

## Issuers and renewal

- **Let's Encrypt:** automatic issuance and renewal. Regional ingress
  hostnames and custom domains use HTTP-01. `contact_email` is optional
  account contact configuration.
- **ZeroSSL:** uses the same lifecycle and also requires `eab_kid` and the
  base64/base64url `eab_hmac_key` in the regional account configuration.
- **Manual:** import a full certificate chain and matching private key through
  the ingress or custom-domain certificate controls. Replace it by importing
  a renewed bundle before expiry; manual certificates are not auto-renewed.

Automatic renewal starts within 30 days of the effective certificate-chain
expiry. The backend records attempt state, a retry delay, expiry and sanitized
failure details. The Console can request issuance or renewal and display the
retry state. While a renewal fails, Nodes retain a still-valid certificate for
the same hostname and certificate identity. Certificate/key mismatches,
hostname mismatches and expired chains are rejected before activation.

Certificates hot-reload without restarting the Node. The private cache under
`serve.artifact_cache_root/certificates` is restored only for the same Node and
region and contains only validated certificate snapshots. Back it up with the
same access restrictions as other private keys. Challenges are not restored
from the certificate cache. Removing a binding or disabling an ingress/TLS
withdraws its certificates and challenges on the next successful snapshot.

For staging tests, operators may set `GRASS_ACME_DIRECTORY_URL` on the Control
API to the test CA's directory URL before creating fresh test certificate
accounts. Existing persisted ACME accounts retain their original directory.
Use dedicated test domains, provider credentials and a disposable approved
database; production CA rate limits apply to ordinary production issuance.

## DNS provider configuration

Regional DNS credentials are write-only and encrypted using the platform
secret. Keep that secret stable and backed up: replacing it without migrating
encrypted data makes stored credentials, ACME accounts and certificate keys
unreadable. Empty credential fields preserve saved
values; a JSON `null` removes a field. Host sources use the same providers for
ordinary A/AAAA/CNAME provisioning, while regional certificates create TXT
challenges separately.

New host-source configuration is encrypted on write. Historical plaintext
configuration is upgraded under a row lock when it is edited or used for DNS
provisioning; API responses expose only the configured field names.

| Provider | Regional challenge configuration | Required DNS access |
| --- | --- | --- |
| Cloudflare | `api_token`, `zone_id`, `zone` | Read the selected zone's records and create/delete challenge TXT records |
| DNSPod | `secret_id`, `secret_key`, `domain`; optional `record_line` | Describe, create, modify and delete records in the selected Tencent Cloud DNSPod domain |
| Route53 | `access_key_id`, `secret_access_key`, `hosted_zone_id`, `zone`; optional signing `region` | `route53:ListResourceRecordSets` and `route53:ChangeResourceRecordSets` for the selected hosted zone |

For a host source also set `record_type` (`A`, `AAAA`, or `CNAME`) and
`record_value`; `ttl` is optional. Enter Route53 zone IDs as `Z…` or
`/hostedzone/Z…`. Its signing region defaults to `us-east-1`.

Route53 updates use atomic `DELETE` and `CREATE` changes to preserve concurrent
TXT values and avoid overwriting a record set changed by another writer.
IAM policies restricting `route53:ChangeResourceRecordSetsActions` must allow
both `CREATE` and `DELETE`; permission restricted to `UPSERT` is insufficient.
Restrict record names/types and the hosted zone ARN according to the DNS
records this deployment manages. Existing TXT values unrelated to the current
challenge are retained.

## Verify entry failover

Check the reserved health endpoint on each private HTTP address, confirm both
entry Nodes have the custom-domain certificate revision, and request the site
through the load balancer:

```sh
curl --fail http://eu-entry-a.internal:8080/_grass/health
curl --fail http://eu-entry-b.internal:8080/_grass/health
curl --fail https://www.example.org/
```

In a test environment, disable one entry backend in HAProxy or stop that entry
Node, wait for the health checks to remove it, and repeat the HTTPS request
using a fresh connection. Restore the entry and confirm it is admitted again.
Place the test deployment on a separate healthy Serve Node to verify the
remaining entry can still Peer Hop to it, including across regions.

This checks entry availability while the assigned deployment Node stays
healthy. Automatic relocation of artifacts when the assigned Serve Node
fails belongs to the separate Serve failover scope.
