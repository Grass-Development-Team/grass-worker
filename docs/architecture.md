# grass-worker 架构设计

当前版本：0.1.0

## 1. 项目目标

`grass-worker` 是一个全新实现的自托管部署平台，第一目标是完整实现类似 Vercel Deployments Page 的部署管理体验，并围绕该页面构建必要的后端、Node、团队、配额和自动域名能力。

项目不是只做一个“能上传静态文件”的工具，而是要形成从团队空间、项目创建、源码配置、部署触发、构建执行、日志查看、产物发布、自动域名、生产/预览环境区分、团队协作到配额限制的完整闭环。

### 1.1 第一目标

第一目标包括以下产品能力：

- Deployment 列表页；
- Deployment 详情页；
- 构建状态展示；
- 构建日志查看；
- 构建产物管理；
- 生产部署与预览部署；
- commit / branch / source revision 信息展示；
- 部署触发；
- 部署取消；
- 部署重试；
- 部署激活；
- 部署回滚；
- 公开访问 URL；
- 平台域名自动分配；
- 实时构建日志（WebSocket）；
- 自动 DNS / 泛域名解析接入抽象；
- Node 对外服务静态站点；
- 团队协作；
- 项目归属与成员权限；
- 团队分组；
- 配额系统；
- 配额检查与用量统计；
- 部署上线审核；
- 审计事件记录；
- SSR 应用类型预留接口。

### 1.2 非第一阶段目标

以下能力不是当前设计的核心目标，可以在后续扩展：

- 在线支付；
- 发票；
- 订阅扣款；
- OAuth 登录；
- GitHub App 深度集成；
- Serverless Functions；
- Edge Runtime；
- 分布式构建缓存；
- 中国境内备案访问控制；
- 备案套餐或备案配额自动放行。

配额系统是第一目标；支付、订阅扣款和商业化结算暂不纳入第一目标。

## 2. 产品范围

## 2.1 Deployments Page

Deployments Page 是本项目的核心页面。页面需要支持用户查看某个项目的全部部署历史，并能快速理解每次部署的来源、状态、产物、日志和可访问地址。

### 列表页要求

Deployment 列表应展示：

- 部署状态；
- 部署环境：Production / Preview；
- 部署来源分支；
- commit hash；
- commit message；
- 触发人；
- 所属项目；
- 创建时间；
- 构建耗时；
- 当前阶段；
- 失败原因摘要；
- 访问 URL；
- 操作入口。

### 详情页要求

Deployment 详情页应展示：

- 总览信息；
- 状态时间线；
- 构建日志；
- 构建阶段；
- 源码信息；
- artifact 信息；
- 环境信息；
- 访问地址；
- 域名绑定状态；
- 错误详情；
- retry / cancel / promote / rollback 操作。

### 状态模型

部署状态分为两层：Build Status（构建生命周期，由 Node 驱动）和 Release Status（上线生命周期，由人/审核策略驱动）。

**Build Status**：

```
pending → claimed → queued → building → ready
                                    ↘ failed
                                    ↘ canceled
```

- `pending`：已创建但尚未被 Node 领取；
- `claimed`：已被 Node 领取但尚未正式执行；
- `queued`：等待资源或配额；
- `building`：构建中；
- `ready`：构建成功并且产物可用；
- `failed`：构建失败；
- `canceled`：被取消。

**Release Status**：

```
draft → pending_review → approved → active
                  ↘ rejected → draft（可重新提交）
```

- `draft`：构建成功，尚未进入上线流程；
- `pending_review`：已提交审核，等待审批；
- `approved`：审核通过，但尚未激活（仅在非自动 activate 场景使用）；
- `rejected`：审核驳回；
- `active`：当前正在 serve 流量。

**状态流转约束**：

- 只有 `build_status = ready` 才能进入 release 流程；
- 同一项目同一环境只能有一个 `active` deployment；
- promote：目标 deployment → `active`，旧 active → `approved`；
- rollback：创建新 release 记录指向旧 deployment，旧 deployment 重新变 `active`。

**三表模型**：

- `deployments`：记录 `build_status` 和 `release_status`（冗余字段，便于列表查询）；
- `deployment_reviews`：审核记录，记录审核人、审核结论、审核时间、原因；
- `releases`：上线时间线，记录哪个 deployment 在什么时间成为 active（含 promote / rollback / auto）。

## 2.2 团队、个人空间与分组

团队是项目和配额的组织边界。为了减少个人项目和团队项目的分支判断，个人空间也作为一种特殊团队处理。

### 团队模型

- 所有项目都归属于 `team_id`；
- 用户注册或创建初始管理员时自动创建个人团队；
- 个人团队 `kind = personal`；
- 普通团队 `kind = team`；
- 个人团队默认只有一个 owner；
- 普通团队可以有多个成员；
- 权限检查统一判断用户是否是团队成员；
- 配额统一绑定到团队；
- 用量统一统计到团队。

### 团队分组

第一版采用团队分组方案，而不是多标签方案。

团队分组用于给不同团队套用不同配额策略，例如：

- `free`；
- `student`；
- `plus`；
- `pro`；
- `ultra`。

建议模型：

- `team_groups` 定义可选分组；
- `teams.group_id` 绑定分组；
- `team_groups.quota_plan_id` 指向默认配额计划；
- 管理员可以调整团队分组；
- 团队分组变更后，配额策略随之变化；
- 如需临时覆盖，可在团队上设置 explicit quota plan，但第一版优先使用 group。

### 团队角色

第一版建议支持：

- `owner`；
- `admin`；
- `member`；
- `viewer`。

权限语义：

- `owner`：拥有团队全部管理权限；
- `admin`：可以管理项目、部署和成员，但不能转移团队所有权；
- `member`：可以创建和部署项目；
- `viewer`：只读访问。

## 2.3 Quota

配额系统是第一目标的一部分，用于限制团队资源使用。

### 配额维度

第一版建议支持：

- 最大项目数；
- 最大团队成员数；
- 最大绑定域名数；
- 每月部署次数；
- 每月构建分钟数；
- 单次构建超时时间；
- artifact 存储总量；
- 单个 artifact 最大大小；
- 并发构建数；
- 最大静态项目数；
- 最大 SSR 项目数；
- 最大 SSR 进程数，预留；
- SSR 月运行小时数，预留。

### 配额行为

- 创建项目时检查项目数配额；
- 邀请成员时检查成员数配额；
- 绑定 Host 时检查 Host 配额；
- 创建 deployment 时检查部署次数配额；
- Node claim 任务前检查并发构建配额；
- 构建完成后记录构建分钟数；
- 上传 artifact 时检查存储配额；
- 超出配额时返回稳定错误码；
- 配额错误需要能在前端页面清晰展示。

### 配额不是支付系统

本项目第一目标只实现配额定义、配额检查、用量统计、订阅管理和配额错误提示，不实现支付、扣款、发票。

### 配额原子性

配额检查和消耗面临并发竞态（check-then-consume），必须原子化处理。

**策略**：Redis 原子操作为主 + PostgreSQL 事件溯源为持久层。

**写入路径**：

```
1. Redis Lua script 原子检查 + 预消耗（INCR 所有相关配额 key）
2. 任一维度超限 → 回滚已 INCR 的 key，返回 QuotaExceeded
3. 业务操作成功 → 写入 quota_events（PG）作为事件溯源
4. 业务操作失败 → Redis DECR 回滚
```

**读取路径**：

```
1. 实时查询：读 Redis（快，允许少量误差）
2. 精确查询：读 quota_usage_counters（PG，异步聚合自 quota_events）
3. 定时校准：每小时从 quota_usage_counters 全量刷新 Redis 计数器
4. 冷启动兜底：Node 重启时从 DB 重建 Redis 计数器
```

**并发构建（信号量模型）**：

```
# 获取槽位
INCR quota:team:{team_id}:concurrent_buildsh
if > max: DECR + return QuotaExceeded

# 释放（构建完成时）
DECR quota:team:{team_id}:concurrent_builds

# 兜底：Redis key 设置 TTL（防止 Node 崩溃导致槽位永久占用）
```

**Redis Lua 复合配额示例**：

```lua
-- 原子检查多个配额维度并预消耗
local limits = cjson.decode(ARGV[1])
for _, limit in ipairs(limits) do
    local current = redis.call("INCR", limit.key)
    if current > limit.max then
        -- 回滚已消耗的
        for _, prev in ipairs(limits) do
            if prev.key == limit.key then break end
            redis.call("DECR", prev.key)
        end
        redis.call("DECR", limit.key)
        return {0, limit.key}
    end
end
return {1}
```

## 2.4 自动域名与 Host Provisioning

自动域名是第一目标。部署成功后必须能得到可访问 URL，否则 Deployments Page 的闭环不完整。

### 基本目标

- 平台可以配置一个或多个 Host Source；
- 项目创建时可以自动分配平台域名；
- Production deployment 可以有稳定域名；
- Preview deployment 可以有唯一预览域名；
- Host Source 可以通过不同方式完成域名配置；
- 具体实现通过 trait 抽象，不把 DNS Provider 逻辑写死在业务代码中。

### Provisioning 方式

第一版至少考虑两种方式：

1. 泛域名解析方式；
2. DNS Provider API 创建记录方式。

泛域名解析方式：

- 平台管理员提前配置 `*.example.com` 指向 Node serve 入口；
- 项目创建时只需要在数据库创建 Host Binding；
- Provisioner 不需要调用外部 DNS API；
- 适合本地、自托管和简单部署。

DNS Provider API 方式：

- 平台配置 DNS Provider 凭据；
- 项目创建或绑定域名时自动创建 DNS record；
- 可支持 Cloudflare、DNSPod、Route53 等；
- Provider 凭据必须通过配置或密钥管理注入，不得提交仓库。

### 自动分配规则

自动分配平台域名必须满足以下条件：

1. 团队仍有 Host 配额；
2. 至少存在一个可用 Host Source；
3. Host Source 必须 `enabled = true`；
4. Host Source 必须 `allows_auto_assign = true`；
5. 如果只有一个可自动分配的 Host Source，直接使用该 source；
6. 如果存在多个可自动分配的 Host Source，必须存在且只能存在一个默认 source；
7. 如果存在多个可自动分配的 Host Source 但没有默认 source，则不自动分配，要求用户或管理员手动选择；
8. 生成的 host 不能与现有 Host Binding 冲突；
9. HostProvisioner 必须返回可追踪结果，成功、pending、failed 都要落库。

建议 `host_sources` 字段包含：

- `id`；
- `kind`：`wildcard` / `dns_provider` / `manual`；
- `label`；
- `base_domain`；
- `enabled`；
- `allows_auto_assign`；
- `is_default`；
- `provider`：例如 `cloudflare` / `dnspod` / `route53` / `none`；
- `config`；
- `created_at`；
- `updated_at`。

默认 source 约束：

- 第一版使用全局默认 Host Source；
- 同一时间最多允许一个 `enabled = true` 且 `is_default = true` 的 Host Source；
- 后续如需区分 preview / production，可以扩展为 `default_for_preview` 和 `default_for_production`。

自动分配失败时不应默认导致项目创建失败。推荐处理方式：

- Host 配额不足或没有可用 source：项目创建成功，但不自动分配 host；
- DNS Provider 临时失败：Host Binding 可以进入 `failed` 或 `pending` 状态，并记录 `host_provision_events`；
- 前端展示可重试入口。

### Host Provisioner Trait

Host 自动配置通过 trait 抽象。调用方只提供项目、团队、部署环境、期望 host 等信息，具体 provisioner 自己决定如何处理。

```host-provisioner.rs
pub trait HostProvisioner {
    async fn provision_project_host(
        &self,
        input: ProvisionProjectHostInput,
    ) -> Result<ProvisionedHost, HostProvisionError>;

    async fn deprovision_project_host(
        &self,
        input: DeprovisionProjectHostInput,
    ) -> Result<(), HostProvisionError>;
}
```

可能的实现：

- `WildcardHostProvisioner`：只创建平台内部绑定，不调用外部 DNS；
- `CloudflareDnsProvisioner`：通过 Cloudflare API 创建 DNS record；
- `ManualHostProvisioner`：只生成待配置状态，提示管理员手动操作；
- `CompositeHostProvisioner`：根据 Host Source 类型选择具体 provisioner。

## 2.5 上线审核与审计

平台需要支持“先审核，后上线”的发布控制能力。

第一阶段要求：

- deployment 构建成功后可以进入待审核状态；
- production promote / activate 必须经过审核；
- 审核通过后才允许成为 production active deployment；
- preview deployment 可以根据项目配置决定是否需要审核；
- 审核行为必须记录审计事件；
- 审计事件至少记录 actor、action、target、timestamp、result、reason。

建议部署发布状态拆分：

- build status：构建状态，例如 `pending` / `building` / `ready` / `failed`；
- release status：上线状态，例如 `draft` / `pending_review` / `approved` / `rejected` / `active`。

审计事件示例：

- `deployment.created`；
- `deployment.build_started`；
- `deployment.build_finished`；
- `deployment.review_requested`；
- `deployment.review_approved`；
- `deployment.review_rejected`；
- `deployment.promoted`；
- `host.provisioned`；
- `quota.denied`。

## 2.6 ICP 说明

ICP 备案属于后续合规能力，第一阶段不实现，也不预留专门字段或接口。本文档仅记录：如果未来面向需要备案的部署环境，需要重新设计备案状态、访问控制和停止页面。

## 3. 系统组成

项目由三个主要运行单元组成：

1. Control API；
2. Node；
3. Web Console。

### 3.1 Control API

Control API 是主后端服务，负责：

- HTTP API；
- setup mode；
- 用户认证；
- 团队管理；
- 分组管理；
- 项目管理；
- 部署管理；
- 配额管理；
- Host Source 管理；
- Host Provisioning 编排；
- 部署上线审核与审计；
- Node 内部 API；
- 本机 Node 进程管理；
- 前端 Console 资源分发。

### 3.2 Node

Node 表示平台节点。第一阶段 Node 必须同时开启 build capability 和 serve capability。

- build capability：执行构建；
- serve capability：对外服务应用。

第一阶段不做 build node 和 serve node 分离调度，避免过早引入节点调度、artifact 跨节点同步、负载均衡和 failover 复杂度。

Node 不直接访问数据库，所有元数据读取和状态变更都通过 Control API 完成。

#### Build Capability

开启 build 后，Node 负责：

- 向 Control API claim deployment；
- 拉取 Git 仓库；
- 执行 install command；
- 执行 build command；
- 通过 Output Adapter 生成 `.grass/output`；
- 生成 `output.toml`；
- 收集 build log；
- 打包 Grass Output artifact；
- 上传 artifact 到当前 Node 对应的本地部署存储；
- 回报部署状态。

#### Serve Capability

开启 serve 后，Node 负责：

- 接收 public HTTP request；
- 根据 Host 查找路由元数据；
- 定位 active production deployment 或 preview deployment；
- 获取 static site artifact；
- 根据路径返回静态文件；
- 支持 directory `index.html`；
- 支持 SPA fallback；
- 设置 cache-control。

第一阶段约束：

- Node 必须同时开启 build 和 serve；
- 本地 build 的 deployment 由同一个本地 Node serve；
- `deployments.node_id` 记录负责该 deployment 的 Node；
- 如果未来支持 build / serve 分离，可以扩展为 `build_node_id` 和 `serve_node_id`；
- 第一阶段启动校验中，如果 `build = false` 或 `serve = false`，Node 应自动修正为 true 并给出警告。

### 3.3 API 与 Node 的配对模式

本项目采用 Node serve 作为标准对外服务模式。

- Control API 负责控制平面；
- Node 负责 build 和 serve；
- 本机部署也应运行 Node；
- Control API 可以配置是否自动启动本地 Node；
- 当启用自动本地 Node 时，API 作为 Node 的进程管理程序；
- 本地 Node 仍然通过内部 API 与 Control API 通信，不共享数据库连接。

推荐配置语义：

```api-node-config.toml
[node_manager]
auto_start_local_node = true
local_node_binary = "grass-node"
local_node_config = "./node.toml"
restart_on_exit = true

[node]
id = "local-node-1"
control_api = "http://127.0.0.1:7817"
node_token = "change-me"
work_root = "/data/node/workspaces"

[node.capabilities]
build = true
serve = true

# 第一阶段 build 和 serve 必须同时为 true。
# 后续版本才允许单独关闭其中一种 capability。

[node.build]
concurrency = 2

[node.serve]
listen = "0.0.0.0:8080"
```

API 管理本地 Node 时，只负责进程生命周期：

- 启动；
- 停止；
- 重启；
- 退出监控；
- graceful shutdown 联动。

API 不应绕过内部协议直接调用 Node 内部函数。

### 3.4 Web Console

Web Console 是管理界面，目标是提供类似 Vercel Deployments Page 的部署体验。

它负责：

- setup flow；
- login；
- team switcher；
- project list；
- project detail；
- deployment list；
- deployment detail；
- build log viewer；
- host management；
- quota usage page；
- team members page；
- admin pages。

### 3.5 实时构建日志（WebSocket）

实时日志通过 WebSocket 传输，Node → Control API → Browser 全链路使用同一协议。

**为什么选 WebSocket**：

- 浏览器原生支持，不需要 gRPC-Web 代理或 GraphQL subscription 中间层；
- Axum 内置 `axum::extract::ws` 支持；
- 双向通信：下行推送日志 + 上行发送 cancel 命令，一个连接搞定。

**日志消息协议（JSON over WebSocket）**：

```
下行（server → client）：
{
  "type": "log",
  "deployment_id": "...",
  "stage": "build",           // install | build | archive | upload
  "line": "...",
  "timestamp": "...",
  "seq": 42
}

{
  "type": "stage_change",
  "deployment_id": "...",
  "stage": "archive"
}

{
  "type": "done",
  "deployment_id": "...",
  "build_status": "ready"
}

上行（client → server）：
{
  "type": "subscribe",
  "deployment_id": "..."
}

{
  "type": "cancel",
  "deployment_id": "..."
}
```

**断线重连**：

- 前端维护 `last_seq`；
- 重连后通过 HTTP `GET .../build-log?after_seq=42` 补拉缺失行；
- 然后继续 WS 接收新行。

**持久化**：

- Node 边构建边写文件边推 WS；
- 构建完成后完整日志文件作为 artifact 上传；
- WS 只是传输通道，不替代存储。

**Node 内部路径**：

```
Node Build Runner
  ├── stdout/stderr → 写 build-log.txt（本地）
  ├── stdout/stderr → 推 WebSocket 帧（行缓冲）
  └── 构建完成 → 打包 build-log.txt → 上传 artifact
```

**API 中继**：

```
Node ──WS──▶ Control API ──WS──▶ Browser
                │
                └── 写入 audit_events（deployment.log_stream_started / log_stream_ended）
```

## 4. 技术栈

## 4.1 后端技术栈

### 语言与工程

- Rust 2024 edition：主开发语言；
- Cargo workspace：管理 Control API、Node 和内部 crates；
- workspace dependencies：统一依赖版本；
- Just `1.50+`：统一开发命令；
- rustfmt：统一 Rust 格式；
- clippy：静态质量检查；
- thiserror：结构化错误；
- async-trait：仅在确实需要 async trait object 时使用。

### Web/API

- Axum：HTTP server、routing、extractor；
- Tower：middleware 和 service abstraction；
- Tower HTTP：trace、CORS、compression、static file 等 HTTP 能力；
- axum-extra：cookie 等扩展 extractor；
- serde / serde_json：请求响应 DTO 序列化；
- mime_guess：静态文件 content type 判断。

### Async / Runtime

- Tokio：async runtime；
- tokio signal：graceful shutdown；
- tokio process：Node build command 执行；
- tokio fs：异步文件读写；
- tokio time：轮询、超时、TTL 相关任务。

### Database

- PostgreSQL：主持久化数据库；
- SeaORM：ORM；
- SeaORM Migration：schema migration；
- UUID：主键、token id、deployment id；
- chrono / time：时间字段；
- transaction：跨表一致性写入，例如 deployment promote、quota consume。

### Cache / Session / Short-term State

- Redis：session、一次性 token、短期状态；
- Redis lock / atomic operation：Node claim、配额计数、一次性 token 消费；
- Redis TTL：session、临时授权和短期缓存过期。

### Auth / Security

- Argon2id：密码哈希；
- rand：安全随机值；
- subtle：常量时间比较；
- sha2：checksum 或非密码场景；
- cookie security：HttpOnly、Secure、SameSite、Path、Max-Age；
- per-node token：Node internal API 认证（第一版就使用独立 token，不使用共享 token）；
- path traversal protection：artifact、static site、build log 路径防护。

### API 鉴权设计

**用户认证（Session + Cookie）**：

- Session ID：256-bit 随机值，`OsRng` 生成，base64url 编码；
- Session 存储：Redis，30 天绝对 TTL + 15 分钟空闲滑动刷新（每次访问刷新 TTL）；
- Cookie 属性：`HttpOnly; Secure; SameSite=Strict; Path=/api; Partitioned`；
- 密码哈希：Argon2id；
- Session 固定防御：登录成功时重新生成 session ID；
- 登录限流：Redis token bucket，5 次/账号/分钟，30 次/IP/分钟；
- CSRF 防御：`SameSite=Strict`（主防线）+ mutation 操作需要 `X-CSRF-Token` header（纵深防御）。

**Node Internal API 认证（Per-Node Token）**：

- Token 生成：API 创建 Node 时自动生成 256-bit 随机 token；
- Token 存储：DB 存 SHA-256 hash，明文仅创建时展示一次；
- Token 验证：constant-time 比较（subtle crate）；
- Token 吊销：API 支持吊销，Redis 黑名单缓存，即时生效；
- Setup 流程创建第一个 Node 时自动生成 token。

**Middleware 链**：

```
TraceLayer → CORS → SessionExtractor → AuthGuard → RateLimit → Router
```

### Runtime / Container Abstraction

- ContainerRuntime trait：统一容器化构建和部署入口；
- Podman socket backend：默认容器运行时后端；
- Docker socket backend：兼容 Docker socket 或 DinD 暴露出来的 socket；
- Apple Container backend：预留 macOS 原生容器后端；
- Jail backend：预留 BSD jail 或类似隔离后端；
- 构建和部署都必须通过运行时抽象执行，不直接把命令裸跑在宿主机上。

### Storage / Artifact

- local filesystem：第一版 artifact 和 static site 存储；
- zip：static site bundle 打包和解包；
- checksum：artifact 完整性记录；
- storage abstraction：后续可扩展 S3 / MinIO / R2。

### DNS / Host Provisioning

- HostProvisioner trait：统一自动域名配置入口；
- Wildcard provisioner：泛域名解析模式；
- DNS provider provisioner：通过 Provider API 创建 DNS record；
- reqwest：调用外部 DNS Provider API。

### Frontend Asset

- rust-embed：嵌入 Web Console 构建产物；
- assets crate：提供统一资源查找；
- `./public` override：运行时覆盖内置资源，便于部署和调试。

### Observability

- tracing：结构化日志；
- tracing-subscriber：日志订阅和格式化；
- TraceLayer：HTTP 请求日志；
- operation 字段：稳定标识错误发生位置。

### HTTP Client

- reqwest：Node 调用 Control API、Control API 调用 DNS Provider；
- rustls：TLS backend；
- timeout / retry：外部 HTTP 调用必须有超时策略。

### 部署

- Docker 多阶段构建；
- runtime 镜像只包含必要二进制和运行依赖；
- 运行时使用非 root 用户；
- 生产部署应使用外部 PostgreSQL、Redis 和可配置 Node。

## 4.2 前端

前端使用 Vite+ 工具链，而不是直接按普通 Vite 项目处理。

- Vite+；
- TypeScript；
- React；
- React Router；
- TanStack Query；
- Tailwind CSS；
- shadcn/ui；
- Bun 作为 Vite+ 选择的底层 package manager；
- 前端开发、检查、测试、构建和依赖管理统一以 Vite+ 的 `vp` 为入口。

## 4.3 UI

UI 使用 `shadcn/ui`。

约定：

- 使用 shadcn/ui 官方默认风格；
- 使用 shadcn/ui 官方 CSS variables 和 Tailwind 主题约定；
- 使用官方 blocks 作为页面基线；
- Dashboard 页面优先参考 `dashboard-01`；
- Login 页面优先参考 `login-04`；
- 组件代码生成到项目内后允许本地维护；
- 不另起一套独立设计系统；
- 初始阶段不大幅魔改官方 block 风格。

说明：shadcn/ui 不是传统意义上通过 npm 包直接运行的封闭组件库，它更接近一套可复制到项目源码中的组件模板、样式变量和设计约定。因此文档中使用“官方默认风格”和“官方 blocks”表达，而不是描述为“引入官方样式包”。

## 5. Vite+ 工具链约定

Vite+ 是 Web 统一工具链，包含全局命令行工具 `vp` 和项目内依赖 `vite-plus`。工具链地址：https://viteplus.dev/

本仓库约定：Console 的项目管理入口是 `vp`，Bun 只作为 `vp` 识别和调用的底层 package manager。文档、Just 命令和 CI 不直接把 `bun run`、`vite`、`vitest` 等作为标准入口。

### 5.1 Vite+ 的目的

Vite+ 的目的不是替代业务框架，而是统一 Web 前端工具链入口。

它将 runtime、package manager、dev server、build、test、lint、format、type-check 和 monorepo task 组织到 `vp` CLI 下，减少多工具配置分裂，并提供更适合人类和 AI 协作的一致命令语义。

Vite+ 解决的问题：

- 避免同时维护 `npm`、`pnpm`、`yarn`、`bun` 命令差异；
- 避免 `vite`、`vitest`、`eslint`、`prettier`、`tsc` 命令分散；
- 使用 `vp check` 统一 format、lint 和 type-check；
- 使用 `vp run` 管理 workspace task；
- 使用 `vite.config.ts` 集中配置；
- 使用 `vp install`、`vp add`、`vp remove` 让 Vite+ 选择正确 package manager；
- 保留 Vite 生态开发体验，但项目工具链文档不按普通 Vite 项目来写。

### 5.2 安装

macOS / Linux 安装：

```viteplus-install.sh
curl -fsSL https://vite.plus | bash
```

安装后打开新的终端会话，执行：

```viteplus-help.sh
vp help
```

### 5.3 创建项目

前端项目应通过 Vite+ 创建。

常用命令：

```viteplus-create.sh
vp create
vp create vite:application
vp create vite -- --template react-ts
vp create --list
```

如果后续需要 monorepo 模板，可以使用：

```viteplus-monorepo.sh
vp create vite:monorepo
```

### 5.4 日常命令

前端开发和依赖管理统一使用：

```viteplus-commands.sh
vp install
vp add <package>
vp remove <package>
vp dev
vp check
vp test
vp build
vp preview
```

命令含义：

- `vp install`：根据项目配置使用正确 package manager 安装依赖，本仓库 Console 由 Bun lockfile / `packageManager` 约束为 Bun；
- `vp add` / `vp remove`：通过 Vite+ 修改依赖，底层由 Bun 执行；
- `vp dev`：启动开发服务器；
- `vp check`：格式化、lint 和类型检查；
- `vp check --fix`：自动格式化并执行可修复 lint；
- `vp test`：运行测试；
- `vp build`：生产构建；
- `vp preview`：本地预览生产构建。

如果需要运行 `package.json` scripts，使用 `vp run <script>`。`vp build` 是 Vite+ 内建 build，不等同于执行 `package.json` 中的 `build` script；如果明确要运行 script，则使用 `vp run build`。

### 5.5 配置文件

Vite+ 使用 `vite.config.ts` 作为统一配置入口。

配置通过 `vite-plus` 导入 `defineConfig`：

```vite.config.ts
import { defineConfig } from 'vite-plus';

export default defineConfig({
  server: {},
  build: {},
  preview: {},
  test: {},
  lint: {},
  fmt: {},
  run: {},
  pack: {},
  staged: {},
});
```

Vite+ 扩展配置项包括：

- `lint`：Oxlint；
- `fmt`：Oxfmt；
- `test`：Vitest；
- `run`：Vite Task；
- `pack`：tsdown；
- `staged`：staged-file checks。

### 5.6 静态检查

推荐开启 type-aware lint 和 type check。

```vite.config.ts
import { defineConfig } from 'vite-plus';

export default defineConfig({
  lint: {
    options: {
      typeAware: true,
      typeCheck: true,
    },
  },
});
```

`vp check` 是前端静态质量检查的主命令，不应默认拆成 `prettier`、`eslint` 和 `tsc` 三套命令。

### 5.7 与后端集成

Control API 只关心 Vite+ 的构建产物，不耦合 Vite+ 内部实现。

开发模式：

- `just run console` 通过 `vp dev` 启动；
- Control API 将前端路由代理到 Vite+ dev server。

生产模式：

- Web Console 通过 `vp build` 构建；
- 构建产物写入约定目录；
- Rust 通过 assets crate 嵌入构建产物；
- 运行时允许 `./public` 覆盖嵌入资源。

## 6. 推荐目录结构

目录结构以当前代码为基准逐步扩展，不为了预设分层一次性创建大量空目录。

```grass-worker-structure.txt
grass-worker/
├── Cargo.toml
├── Cargo.lock
├── Justfile
├── rustfmt.toml
├── config.toml.example
├── apps/
│   ├── control-api/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── init.rs
│   │       ├── state.rs
│   │       ├── domain/
│   │       │   ├── mod.rs
│   │       │   ├── users.rs
│   │       │   ├── teams.rs
│   │       │   ├── projects.rs
│   │       │   ├── deployments.rs
│   │       │   ├── quotas.rs
│   │       │   ├── hosts.rs
│   │       │   ├── nodes.rs
│   │       │   ├── audits.rs
│   │       │   └── settings.rs
│   │       ├── features/
│   │       │   ├── mod.rs
│   │       │   ├── router.rs
│   │       │   ├── frontend.rs
│   │       │   ├── actions/
│   │       │   │   ├── mod.rs
│   │       │   │   ├── reset_password.rs
│   │       │   │   └── verify_email.rs
│   │       │   └── api/
│   │       │       ├── mod.rs
│   │       │       ├── v1.rs
│   │       │       └── v1/
│   │       │           ├── auth.rs
│   │       │           ├── auth/
│   │       │           │   ├── login.rs
│   │       │           │   ├── logout.rs
│   │       │           │   └── register.rs
│   │       │           ├── user.rs
│   │       │           ├── user/
│   │       │           │   ├── info.rs
│   │       │           │   └── settings.rs
│   │       │           ├── team.rs
│   │       │           ├── team/
│   │       │           │   ├── members.rs
│   │       │           │   └── invitations.rs
│   │       │           ├── project.rs
│   │       │           ├── project/
│   │       │           │   └── hosts.rs
│   │       │           ├── deployment.rs
│   │       │           ├── deployment/
│   │       │           │   ├── logs.rs
│   │       │           │   └── artifacts.rs
│   │       │           ├── quota.rs
│   │       │           ├── host.rs
│   │       │           ├── node.rs
│   │       │           └── internal.rs
│   │       └── infra/
│   │           ├── mod.rs
│   │           ├── config/
│   │           ├── database/
│   │           │   ├── mod.rs
│   │           │   ├── connection.rs
│   │           │   ├── migrate.rs
│   │           │   ├── seed.rs
│   │           │   ├── entity/
│   │           │   │   ├── mod.rs
│   │           │   │   ├── users.rs
│   │           │   │   ├── teams.rs
│   │           │   │   ├── projects.rs
│   │           │   │   ├── deployments.rs
│   │           │   │   ├── quotas.rs
│   │           │   │   ├── hosts.rs
│   │           │   │   ├── nodes.rs
│   │           │   │   ├── audits.rs
│   │           │   │   └── settings.rs
│   │           │   └── migration/
│   │           ├── logger/
│   │           ├── http/
│   │           ├── redis/
│   │           ├── storage/
│   │           ├── host_provision/
│   │           └── node_manager/
│   ├── node/
│   └── console/
│       ├── index.html
│       ├── package.json
│       ├── tsconfig.json
│       ├── vite.config.ts
│       ├── components.json
│       └── src/
│           ├── main.tsx
│           ├── App.tsx
│           ├── router.tsx
│           ├── styles.css
│           ├── vite-env.d.ts
│           ├── lib/
│           │   ├── api.ts
│           │   ├── utils.ts
│           │   └── constants.ts
│           ├── components/
│           │   └── ui/
│           │       ├── button.tsx
│           │       ├── card.tsx
│           │       ├── field.tsx
│           │       ├── input.tsx
│           │       ├── label.tsx
│           │       ├── separator.tsx
│           │       ├── sheet.tsx
│           │       ├── sidebar.tsx
│           │       ├── skeleton.tsx
│           │       └── tooltip.tsx
│           ├── hooks/
│           │   └── use-mobile.tsx
│           ├── layouts/
│           │   ├── auth-layout.tsx
│           │   └── app-layout.tsx
│           └── features/
│               ├── setup/
│               │   ├── setup-route.tsx
│               │   ├── setup.api.ts
│               │   └── components/
│               │       ├── step-indicator.tsx
│               │       ├── database-step.tsx
│               │       ├── admin-step.tsx
│               │       ├── site-step.tsx
│               │       ├── node-step.tsx
│               │       ├── storage-step.tsx
│               │       └── finish-step.tsx
│               ├── auth/
│               │   ├── login-route.tsx
│               │   ├── login-form.tsx
│               │   └── auth.api.ts
│               ├── dashboard/
│               │   └── dashboard-route.tsx
│               ├── teams/
│               ├── projects/
│               ├── deployments/
│               ├── hosts/
│               ├── quota/
│               └── admin/
├── crates/
│   ├── assets/
│   ├── config/
│   ├── crypto/
│   ├── session/
│   ├── token/
│   ├── archive/
│   ├── node-protocol/
│   └── validator/
└── docs/
```

`features/api/` 下的文件路径必须直接镜像 API path。也就是说，去掉 `/api/` 前缀后，剩余 path 用 Rust module path 表达：

- `/api/v1` 的聚合 router 写在 `features/api/v1.rs`；
- `/api/v1/user` 对应 `features/api/v1/user.rs`；
- `/api/v1/user/info` 对应 `features/api/v1/user/info.rs`；
- `/api/v1/team` 对应 `features/api/v1/team.rs`；
- `/api/v1/team/members` 对应 `features/api/v1/team/members.rs`；
- `/api/v1/deployment/logs` 对应 `features/api/v1/deployment/logs.rs`。

Router 写在当前目录的最顶层 path 文件里。例如 `features/api/v1/user.rs` 负责 `/api/v1/user` 以及合并或挂载 `user/info.rs`、`user/settings.rs`；`features/api/v1.rs` 负责合并 `v1/auth.rs`、`v1/user.rs`、`v1/team.rs` 等模块，并 nest 到 `/v1`。`features/api/mod.rs` 再合并版本级 router，并 nest 到 `/api` 或交给上层统一挂载。

`features/router.rs` 和 `features/frontend.rs` 结构应保留。新增 API 时不要按资源预先创建 `team/route.rs`、`team/request.rs`、`team/response.rs` 这类分层目录；应按 URL path 创建对应 `.rs` 文件。单个 path 文件内部可以包含 controller、请求结构、响应结构、service struct、service method、校验和错误映射。

### 6.1 Console 目录架构

Console 采用与后端 VSA 一致的 feature-based 垂直切片组织方式。每个 feature 目录内聚该业务域的路由、组件和 API 调用。

**分层约定**：

| 层级 | 目录 | 职责 | 能否包含业务逻辑 |
|------|------|------|:---:|
| UI 原语 | `components/ui/` | shadcn/ui 复制过来的组件模板，纯展示 | 否 |
| 共享工具 | `lib/` | 通用 API helper、`cn()`、常量 | 否（不耦合具体 API endpoint） |
| 共享 hook | `hooks/` | 跨 feature 复用的 hook | 否（不耦合具体 feature 类型） |
| 布局 | `layouts/` | 页面级布局壳（sidebar、header），渲染 `<Outlet />` | 仅布局逻辑 |
| Feature | `features/<name>/` | 业务 feature 的完整闭环 | 是 |

**Feature 内部结构**：

```
features/<name>/
├── <name>-route.tsx          # 路由入口组件（必需）
├── <name>.api.ts             # 该 feature 专用的 API 调用封装（可选）
└── components/               # 该 feature 专用的 UI 组件（可选）
```

**规则**：

- 路由文件命名 `<feature>-route.tsx`，放在 feature 目录顶层。
- API 文件命名 `<feature>.api.ts`，与 `lib/api.ts`（通用 helper）区分。

  **API 去中心化**：`lib/api.ts` **仅**包含通用 `request()` 函数和 `ApiResponse<T>` 类型，**不得**包含任何 feature-specific 的 API 方法或业务类型。每个 feature 的 API 调用封装在其自身的 `<feature>.api.ts` 中（例如 `features/setup/setup.api.ts` 包含 `setupApi.getSetupState`、`setupApi.configureDatabase` 等）。所有调用方必须从所属 feature 的 `.api.ts` 导入，不得从 `lib/api.ts` 导入业务 API。

  若 API 调用简单可直接写在 route 中，不必强制创建 `.api.ts` 文件。

- Feature 内组件放在 `components/` 子目录下，不跨 feature 共享。需要共享时提升到 `components/`（非 `ui/`）或 `lib/`。
- 类型定义就近放在使用它们的文件中，不建全局 DTO 目录。
- 后续 feature 按需创建目录，不预建空壳。

**布局与路由**：

布局组件使用 `<Outlet />` 渲染子路由：
- `auth-layout.tsx`：公开路由壳（setup、login 等未认证页面）。
- `app-layout.tsx`：认证后路由壳（sidebar 导航 + team switcher + 内容区）。

`router.tsx` 按布局分组：

```tsx
<Routes>
  <Route element={<AuthLayout />}>
    <Route path="/setup" element={<SetupRoute />} />
    <Route path="/login" element={<LoginRoute />} />
  </Route>
  <Route element={<AppLayout />}>
    <Route path="/" element={<DashboardRoute />} />
    {/* 后续 feature 路由在此嵌套 */}
  </Route>
</Routes>
```

**与后端的对应关系**：

| Console Feature | 后端 API Slice | 后端 Domain |
|-----------------|---------------|-------------|
| `features/setup/` | `features/api/v1/setup.rs` | — |
| `features/auth/` | `features/api/v1/auth.rs` | `domain::users` |
| `features/dashboard/` | — | — |
| `features/teams/` | `features/api/v1/team.rs` | `domain::teams` |
| `features/projects/` | `features/api/v1/project.rs` | `domain::projects` |
| `features/deployments/` | `features/api/v1/deployment.rs` | `domain::deployments` |
| `features/hosts/` | `features/api/v1/host.rs` | `domain::hosts` |
| `features/quota/` | `features/api/v1/quota.rs` | `domain::quotas` |
| `features/admin/` | 跨多个 admin API | 跨多个 domain |

## 7. 后端架构

Control API 采用 VSA（Vertical Slice Architecture）为主、少量横向模块辅助的结构。请求入口按 router/path 组织，通用数据库业务能力按 domain 模块组织，框架和外部系统细节放在 infra。

```backend-flow.txt
Client / Node / Public Visitor
  ↓
HTTP Router / Middleware / Extractor
  ↓
features/api/v1/<path>.rs controller
  ↓
Service method on request/service struct
  ↓
domain::<module> database-backed business functions
  ↓
infra::database::entity + PostgreSQL / Redis / Storage / DNS Provider
```

### 7.1 Features / API Slice

`features` 是主要请求组织方式，当前目录结构以 router 为中心：

- `features/router.rs` 聚合 Control API 的所有 HTTP 路由，包括 API 和 Console 静态资源；
- `features/api/v1.rs` 聚合 `/api/v1/...` 路由，合并 `features/api/v1/<path>.rs` 提供的 router；
- `features/api/v1/<path>.rs` 或 `features/api/v1/<path>/<subpath>.rs` 直接对应 API path，例如 `/api/v1/user` → `v1/user.rs`，`/api/v1/user/info` → `v1/user/info.rs`；
- router 写在当前 path 的顶层文件中，例如 `v1/user.rs` 合并或挂载 `v1/user/info.rs`；
- `features/frontend.rs` 负责 Console 资源分发；
- `features/actions/<action>.rs` 对应 action path，例如 `/actions/reset-password` → `features/actions/reset_password.rs`。Action 也是前端入口，但通常服务于邮件链接、一次性 token、验证/重置等需要页面承载的 API 调用流程。

单个 API slice 文件可以同时包含：

- Axum controller / handler；
- 请求结构；
- 响应结构；
- service struct；
- service method，例如 `RegisterService::register`；
- 当前 API flow 的校验、事务编排、错误映射和测试。

本项目不设置全局 DTO 层。请求和响应结构应就近定义在对应 API slice 文件或模块中。命名上不强调 DTO，可按语义使用 `RegisterService`、`SetupStateResponse`、`CreateProjectService` 等名称。

API slice 可以调用多个 domain 函数、Redis token 服务、storage、host provisioner、mailer 和配置对象完成一次请求。完整 HTTP flow 不应下沉到 domain。

### 7.2 Domain 模块

本项目的 `domain` 不是纯 DDD 领域模型层，而是 database-backed business modules。

`domain` 可以直接依赖 SeaORM、`infra/database/entity` 和数据库连接 trait，用于封装跨 API slice 复用的数据库业务函数，例如：

- `users::get_user_by_id`；
- `users::get_user_by_email`；
- `users::create_user`；
- `teams::create_team`；
- `teams::add_team_member`；
- `projects::get_project_by_id`；
- `deployments::append_deployment_event`；
- `quotas::consume_quota`；
- `audits::create_audit_event`。

`domain` 模块可以包含：

- 查询函数；
- 基础写入函数；
- 与数据库模型紧密相关的参数结构，例如 `CreateUserParams`；
- 数据库错误转换；
- 业务状态 helper，例如账号状态、部署状态、团队角色判断；
- 接受 `ConnectionTrait` 的函数，以便同时支持普通连接和事务。

`domain` 不负责：

- Axum handler；
- router；
- HTTP extractor；
- cookie/header 写入；
- 完整 API flow，例如 login、register、finish setup、retry deployment；
- Console 页面逻辑。

### 7.3 Infra / Database Entity

SeaORM entity 放在 `infra/database/entity/`，不放在 `domain/`。

`infra/database/entity` 只负责表映射、relation 和 SeaORM 需要的 active model 定义。entity 不承载完整业务流程，也不负责 HTTP 响应。

`infra/database/migration/` 继续负责 schema migration。migration 与 entity 需要保持一致，但不要求每次迁移都立即生成额外抽象层。

### 7.4 Infra 层

Infra 层封装外部系统和框架细节。

- `config/`：配置结构、默认值、文件读写、ENV override、配置检查；
- `database/`：连接初始化、迁移、entity、数据库错误类型、系统初始化数据；
- `redis/`：Redis client、lock、短期状态；
- `http/`：extractor、middleware、response serializer、CORS、cookie 工具；
- `host_provision/`：泛域名、DNS Provider、手动模式等 host provisioner；
- `logger/`：tracing subscriber 和日志格式；
- `node_manager/`：本地 Node 进程启动、停止、重启、监控；
- `storage/`：artifact 和 static site 存储；
- `session.rs`：会话常量和会话基础配置；
- `error.rs`：服务内部统一错误类型和错误分类；
- `utils/`：仅放确实跨模块复用且稳定的小工具。

### 7.5 Internal Crates 层

内部 crates 提供稳定、低业务耦合的能力。

- 主服务依赖内部 crates；
- 内部 crates 不应反向依赖主服务；
- 内部 crates 应尽量提供小而清晰的公开 API；
- 涉及安全、令牌、密码、序列化等能力时，必须有单元测试覆盖关键路径。

## 8. 启动顺序

### 8.1 Control API 启动顺序

推荐启动顺序：

1. 初始化日志；
2. 读取配置文件；不存在时生成默认配置并进入 setup 或提示用户补齐；
3. 合并 ENV override；
4. 校验配置并写回补全后的配置；
5. 判断 setup mode 或 ready mode；
6. 初始化数据库连接；
7. 根据配置决定是否自动执行 migration；
8. 初始化基础数据，例如系统权限、默认团队分组、默认个人团队基线、默认配额计划；
9. 初始化 Redis client；
10. 初始化 storage；
11. 初始化 HostProvisioner；
12. 根据配置初始化可选基础设施，例如 SMTP mailer；
13. 根据配置决定是否自动启动本地 Node；
14. 构建 `AppState` 并注入路由；
15. 绑定监听地址并启动 Axum server；
16. 监听 `Ctrl+C` 或 `SIGTERM`；
17. 执行 graceful shutdown，包括停止本地 Node、关闭连接池和释放资源。

### 8.2 Node 启动顺序

推荐启动顺序：

1. 初始化日志；
2. 读取 Node 配置；
3. 校验 node token、control API URL、capabilities；
4. 初始化 Control API client；
5. 向 Control API 注册 Node 信息（`POST /api/v1/internal/nodes/register`）；
6. 启动心跳 loop（每 30s `POST /api/v1/internal/nodes/heartbeat`）；
7. 如果开启 build capability，初始化 workspace 和 build loop；
8. 如果开启 serve capability，初始化 serve router、metadata cache 和 storage resolver；
9. 启动 build loop 和/或 serve HTTP server；
10. 监听 `Ctrl+C`、`SIGTERM` 或父进程关闭信号；
11. graceful shutdown，停止 heart 跳、停止 claim 新任务，等待当前任务收尾或按配置取消。

### 8.3 Node 注册与心跳

**注册**：

- Node 启动后调用 `POST /api/v1/internal/nodes/register`；
- 携带 `Authorization: Bearer <node_token>` header；
- Body：`{ node_id, capabilities, version }`；
- API 验证 token 后写入/更新 `nodes` 表。

**心跳**：

- 每 30s 调用 `POST /api/v1/internal/nodes/heartbeat`；
- API 更新 `nodes.last_heartbeat_at`；
- API 定时检查心跳超时（>90s 无心跳）的 Node，标记为 `unhealthy`。

**安全要点**：

- Per-node token：每个 Node 独立 token，存储在 `nodes` 表（SHA-256 hash）；
- Token 泄露时可通过 API 吊销单个 Node 的 token，不影响其他 Node；
- Node ID 由管理员在 Console 创建 Node 时指定，API 生成对应 token。

## 9. 数据库与迁移

### 9.1 基线要求

- PostgreSQL 作为主持久化数据库；
- SeaORM entity 放在 `infra/database/entity/`；
- migration 放在 `infra/database/migration/`；
- 系统内置数据初始化必须幂等；
- 写操作需要跨表一致性时使用事务；
- 表建议包含 `created_at`、`updated_at`；
- 需要软删除时包含 `deleted_at`。

### 9.2 Migration 策略

启动时可以自动执行 migration，但生产环境是否自动迁移应由部署策略决定。

建议配置项：

```migration-config.toml
[database]
auto_migrate = true
```

要求：

- 开发环境可以默认自动 migration；
- 生产环境可以关闭自动 migration；
- 必须提供统一命令手动执行 migration；
- 不要求用户直接使用 `seaorm-cli`；
- 提供 `<service> migrate`（比如编译出的主程序产物叫 `gworker`，那就是 `gworker migrate`）；
- migration 失败时服务启动应失败并输出明确错误；
- migration 不应吞掉错误继续启动。

### 9.3 Seed 策略

系统初始化数据必须幂等，包括：

- 默认团队分组；
- 默认配额计划；
- 默认权限或角色；
- 默认个人团队基线；
- 默认 Host Policy。

Seed 不创建默认管理员，也不创建无 owner 的个人团队。初始管理员必须由 setup flow 注册；注册完成后，setup/admin 应创建该管理员的个人团队，并将管理员加入为 owner。

Seed 不创建 Host Source。Host Source 是用户或平台管理员提供的 Runtime Setting，应由 setup flow 或后续 Host Source 管理能力创建；如果没有可用 Host Source，项目创建不失败，但不自动分配平台域名。

## 10. 数据模型

### 10.1 用户、团队与分组

核心表：

- `users`；
- `user_password_credentials`；
- `sessions` 或 Redis session；
- `teams`；
- `team_groups`；
- `team_members`；
- `team_invitations`。

`teams` 建议字段：

- `id`；
- `kind`：`personal` / `team`；
- `group_id`；
- `slug`；
- `name`；
- `owner_user_id`；
- `created_at`；
- `updated_at`；
- `deleted_at`。

### 10.2 配额

核心表：

- `quota_plans`；
- `quota_limits`；
- `quota_usage_counters`；
- `quota_events`。

建议模型：

- `quota_plans` 定义 Free / Student / Internal 等配额包；
- `quota_limits` 定义每个 plan 的限制项；
- `team_groups.quota_plan_id` 将团队分组绑定到默认 plan；
- `quota_usage_counters` 保存周期性用量；
- `quota_events` 保存用量变更事件，便于审计和重算。

### 10.3 项目与部署

核心表：

- `projects`；
- `deployments`；
- `deployment_events`；
- `deployment_artifacts`；
- `deployment_reviews`；
- `audit_events`；
- `releases`。

`projects` 必须通过 `team_id` 归属团队，不直接归属个人用户。

`projects` 和 `deployments` 应包含 `runtime_kind`，第一阶段支持 `static` 和 `ssr` 两种枚举值；`ssr` 先返回未实现错误，不进入实际 serve。

### 10.4 Host

核心表：

- `host_sources`；
- `project_host_bindings`；
- `host_policies`；
- `host_provision_events`。

`host_sources` 至少需要表达：

- 泛域名解析 source；
- DNS Provider source；
- 手动 source。

### 10.5 Node

核心表：

- `nodes`；
- `node_capabilities` 或 capabilities JSON；
- `node_heartbeats`，也可以只用 Redis 短期状态。

Node 主要用于观测和任务分配，不直接作为业务 owner。

## 11. API 设计

### 11.1 System API

- `GET /health`
- `GET /api/v1/info`

### 11.2 Setup API

- `GET /api/v1/setup/state`
- `POST /api/v1/setup/database`
- `POST /api/v1/setup/admin`
- `POST /api/v1/setup/site`
- `POST /api/v1/setup/node`
- `POST /api/v1/setup/storage`，默认使用 `/data`，通常无需用户修改；
- `POST /api/v1/setup/finish`

Setup flow 必须覆盖基础系统配置：database、initial admin、site config、first node、secret key。`POST /api/v1/setup/admin` 负责注册初始管理员，并创建管理员的个人团队和 owner 成员关系；seed 不创建默认管理员，也不创建无 owner 的个人团队。Host Source 由 setup flow 或后续 Host Source 管理能力创建，不由 seed 创建。storage root 在 `config.toml` 中配置，默认值为 `/data`，setup 页面默认填入 `/data`。非关键 Runtime Settings 可以在 setup 中跳过，之后通过 Console 配置。

### 11.3 Auth API

- `POST /api/v1/auth/register`
- `POST /api/v1/auth/login`
- `POST /api/v1/auth/logout`
- `GET /api/v1/me`

注册必须在单一数据库事务中创建用户、密码凭据、个人团队和 owner 成员关系。`signup.policy` 支持 `open`、`invite_only` 和 `closed`；默认 seed 使用 `open`。携带 invitation token 注册时，注册邮箱必须匹配邀请邮箱，并在同一事务中接受邀请。注册成功后直接创建 session。

### 11.4 Team API

- `GET /api/v1/teams`
- `POST /api/v1/teams`
- `GET /api/v1/teams/{team_id}`
- `PATCH /api/v1/teams/{team_id}`
- `GET /api/v1/teams/{team_id}/members`
- `POST /api/v1/teams/{team_id}/invitations`
- `POST /api/v1/team-invitations/accept`
- `PATCH /api/v1/teams/{team_id}/members/{user_id}`
- `DELETE /api/v1/teams/{team_id}/members/{user_id}`

### 11.5 Team Group API

- `GET /api/v1/admin/team-groups`
- `POST /api/v1/admin/team-groups`
- `PATCH /api/v1/admin/team-groups/{group_id}`
- `POST /api/v1/admin/teams/{team_id}/group`

### 11.6 Quota API

- `GET /api/v1/teams/{team_id}/quota`
- `GET /api/v1/teams/{team_id}/quota/usage`
- `GET /api/v1/admin/quota-plans`
- `POST /api/v1/admin/quota-plans`
- `PATCH /api/v1/admin/quota-plans/{plan_id}`

### 11.7 Project API

- `POST /api/v1/projects`
- `GET /api/v1/projects`
- `GET /api/v1/projects/{project_id}`
- `PATCH /api/v1/projects/{project_id}`
- `POST /api/v1/projects/{project_id}/archive`
- `POST /api/v1/projects/{project_id}/unarchive`
- `POST /api/v1/projects/{project_id}/delete`
- `POST /api/v1/projects/{project_id}/restore`
- `POST /api/v1/projects/{project_id}/transfer-team`
- `POST /api/v1/projects/{project_id}/hard-delete`

### 11.8 Deployment API

- `POST /api/v1/projects/{project_id}/deployments`
- `GET /api/v1/projects/{project_id}/deployments`
- `GET /api/v1/projects/{project_id}/deployments/{deployment_id}`
- `POST /api/v1/projects/{project_id}/deployments/{deployment_id}/cancel`
- `POST /api/v1/projects/{project_id}/deployments/{deployment_id}/retry`
- `POST /api/v1/projects/{project_id}/deployments/{deployment_id}/promote`
- `GET /api/v1/projects/{project_id}/deployments/{deployment_id}/build-log`
- `GET /api/v1/projects/{project_id}/deployments/{deployment_id}/artifacts`
- `GET /api/v1/projects/{project_id}/deployments/{deployment_id}/events`

### 11.9 Deployment Review API

- `POST /api/v1/projects/{project_id}/deployments/{deployment_id}/review/request`
- `POST /api/v1/projects/{project_id}/deployments/{deployment_id}/review/approve`
- `POST /api/v1/projects/{project_id}/deployments/{deployment_id}/review/reject`

### 11.10 Host API

- `GET /api/v1/projects/{project_id}/hosts`
- `POST /api/v1/projects/{project_id}/hosts`
- `PATCH /api/v1/projects/{project_id}/hosts/{host_id}`
- `DELETE /api/v1/projects/{project_id}/hosts/{host_id}`
- `POST /api/v1/projects/{project_id}/hosts/{host_id}/primary`
- `POST /api/v1/projects/{project_id}/hosts/{host_id}/provision`

### 11.11 Host Source API

- `GET /api/v1/admin/host-sources`
- `POST /api/v1/admin/host-sources`
- `PATCH /api/v1/admin/host-sources/{source_id}`
- `DELETE /api/v1/admin/host-sources/{source_id}`

### 11.12 Node API

- `GET /api/v1/admin/nodes`
- `GET /api/v1/admin/nodes/{node_id}`
- `GET /api/v1/admin/nodes/{node_id}/health`

### 11.13 Audit API

- `GET /api/v1/admin/audit-events`
- `GET /api/v1/teams/{team_id}/audit-events`

### 11.14 Internal Node API

- `POST /api/v1/internal/nodes/register`
- `POST /api/v1/internal/nodes/heartbeat`
- `POST /api/v1/internal/deployments/claim`
- `POST /api/v1/internal/deployments/{deployment_id}/stage`
- `PUT /api/v1/internal/deployments/{deployment_id}/build-log`
- `PUT /api/v1/internal/deployments/{deployment_id}/static-site`
- `GET /api/v1/internal/serve/resolve-host`

## 12. 请求处理流程

典型 API 请求应遵循：

1. Router 匹配版本化路径；
2. Middleware 执行 CORS、权限等横切检查；
3. Extractor 提取登录态、访客态、JSON body、cookie 等上下文；
4. Controller 调用 feature 业务流程；
5. Feature 调用 domain、quota、token、storage、host provisioner、config 等能力；
6. Domain 访问 PostgreSQL，token/session/lock 访问 Redis；
7. 业务错误映射为统一响应体和合理 HTTP status；
8. 基础设施错误记录详细日志，对客户端返回受控错误信息。

## 13. 响应与错误规范

统一成功响应：

```api-response.json
{
  "code": 200,
  "message": "OK",
  "data": {}
}
```

统一错误响应：

```api-error-response.json
{
  "code": 40001,
  "message": "quota exceeded: monthly deployments limit reached",
  "data": null,
  "trace_id": "optional-trace-id"
}
```

错误类型：

- `Validation`；
- `Unauthorized`；
- `Forbidden`；
- `NotFound`；
- `Conflict`；
- `QuotaExceeded`；
- `Infrastructure`；
- `Internal`。

每个错误应包含：

- `op`；
- `kind`；
- `message`；
- `source`。

错误处理约定：

- 使用统一 `AppError` 表达服务内部错误；
- 使用固定 `op` 标识错误发生位置，例如 `deployment.claim.lock`；
- 基础设施错误必须记录 source，避免丢失排查信息；
- 对客户端返回稳定 code 和 message，不直接泄露数据库、Redis、DNS Provider 等底层错误细节；
- HTTP status 应和业务 code 对齐，例如认证失败返回 `401`，权限不足返回 `403`，资源冲突返回 `409`。

## 14. 配置管理

配置分为 Bootstrap Config 和 Runtime Settings。

Bootstrap Config 使用 TOML + ENV override，只放服务启动前必须知道的配置，例如数据库、Redis、监听地址、密钥、本地 Node 启动方式等。Runtime Settings 存入数据库，由 Console 管理，例如 Host Source、默认 Host Source、配额计划、团队分组、上线审核策略、站点基础信息等。

Setup 页面需要具备配置基础系统的能力。必须填写或确认：

- database；
- initial user / admin；
- site config，例如 site name、site url、public base url；
- first node，例如 node id、serve url、runtime socket；
- storage root，默认 `/data`；
- secret key。

可选项：

- Redis，若未启用 Redis，第一阶段可以使用受限的本地/数据库 fallback，但生产环境推荐配置 Redis；
- node manager 自动启动策略；
- migration 策略；
- 默认 Host Source；
- 默认团队分组；
- 默认配额计划；
- 上线审核策略。

部分非关键 Runtime Settings 可以跳过，后续在 Console 中补充。

Bootstrap Config 推荐区块：

- `[server]`；
- `[database]`；
- `[redis]`；
- `[session]`；
- `[storage]`；
- `[secrets]`；
- `[node_manager]`；
- `[log]`；
- `[migration]`。

ENV 前缀建议使用：

- API 专属环境变量前缀：`GWAPI_`；
- Node 专属环境变量前缀：`GWNODE_`；
- 通用环境变量：`LOG_LEVEL`、`TZ`。

常用 API 环境变量：

- `GWAPI_SERVER_LISTEN`；
- `GWAPI_DATABASE_URL`；
- `GWAPI_REDIS_URL`；
- `GWAPI_STORAGE_ROOT`；
- `GWAPI_SECRET_KEY`；
- `GWAPI_NODE_MANAGER_AUTO_START_LOCAL_NODE`；
- `GWAPI_NODE_MANAGER_LOCAL_NODE_BINARY`；
- `GWAPI_NODE_MANAGER_LOCAL_NODE_CONFIG`；
- `GWAPI_SECRET_KEY`；
- `GWAPI_LOG_LEVEL`。

常用 Node 环境变量：

- `GWNODE_ID`；
- `GWNODE_CONTROL_API`；
- `GWNODE_NODE_TOKEN`；
- `GWNODE_WORK_ROOT`；
- `GWNODE_BUILD_CONCURRENCY`；
- `GWNODE_BUILD_COMMAND_TIMEOUT_SECONDS`；
- `GWNODE_SERVE_LISTEN`；
- `GWNODE_SERVE_PUBLIC_BASE_URL`；
- `GWNODE_LOG_LEVEL`。

优先级：

1. 通用共识 ENV（如 `TZ` / `LOG_LEVEL`）；
2. 服务专属 ENV（如 `GWAPI_` 或 `GWNODE_` 开头的环境变量）；
3. TOML；
4. 默认值。

日志配置优先级：

- API：`LOG_LEVEL` > `GWAPI_LOG_LEVEL` > API TOML；
- Node：`LOG_LEVEL` > `GWNODE_LOG_LEVEL` > Node TOML。

配置要求：

- Control API Bootstrap Config 文件建议为 `config.toml`；
- Node Bootstrap Config 文件建议为 `node.toml`；
- Host Source、Quota Plan、Team Group、Site Config、审核策略等 Runtime Settings 存入数据库；
- Runtime Settings 由 Console 管理；
- 敏感 Runtime Settings 必须加密存储，或继续要求通过 ENV / TOML 注入；
- 配置结构使用 serde 序列化和反序列化；
- 所有字段提供明确默认值或显式必填策略；
- 启动时执行配置检查；
- 密码、密钥、DNS Provider token、SMTP 密码等敏感配置不得写入仓库；
- 环境变量适合覆盖运行时行为。

### 14.1 Control API 配置示例

```config.toml
[server]
listen = "127.0.0.1:7817"

[database]
url = "postgres://postgres:postgres@127.0.0.1:5432/grass_worker"

[redis]
url = "redis://127.0.0.1:6379/0"

[storage]
root = "/data"

[secrets]
secret_key = "change-me"

[session]
cookie_secure = false
session_ttl_seconds = 2592000

[node_manager]
auto_start_local_node = true
local_node_binary = "grass-node"
local_node_config = "./node.toml"
restart_on_exit = true

[migration]
auto_migrate = true

[log]
level = "info"
```

### 14.2 Node 配置示例

```node.toml
[node]
id = "local-node-1"
control_api = "http://127.0.0.1:7817"
node_token = "change-me"
work_root = "/data/node"

[node.capabilities]
build = true
serve = true

[build]
concurrency = 1
command_timeout_seconds = 600
retain_workspace_on_failure = true

[serve]
listen = "0.0.0.0:8080"
public_base_url = "http://127.0.0.1:8080"
metadata_cache_ttl_seconds = 30
artifact_cache_root = "/data/node/artifacts"

[security]
allow_private_repository = false

[development]
verbose_build_log = true
```

### 14.3 Runtime Settings

Runtime Settings 存储在数据库中，由 Console 管理。建议使用领域表优先、通用设置表补充的方式。

领域表：

- `host_sources`；
- `quota_plans`；
- `quota_limits`；
- `team_groups`；
- `release_review_policies`；
- `site_settings`。

通用表：

- `system_settings`。

`system_settings` 建议字段：

- `key`；
- `value`；
- `value_type`；
- `scope`；
- `is_secret`；
- `updated_at`；
- `updated_by`。

Runtime Settings 示例：

- 站点名称；
- 站点公开 URL；
- 默认团队分组；
- 默认配额计划；
- 默认 Host Source；
- 是否启用自动分配域名；
- 是否启用上线审核；
- artifact retention；
- deployment retention；
- signup policy：`open`、`invite_only` 或 `closed`，默认 seed 为 `open`；
- site name / site url。

## 15. Grass Output API

第一阶段支持各框架的 SSG / static 输出，不支持 SSR、ISR、Serverless、Edge、Middleware 等需要运行时计算的能力。框架支持不是通过框架名直接决定 runtime，而是通过框架配置 detector 和 build output inspector 判断最终输出是否能归一化为 `runtime.kind = "static"`。

第一阶段目标：

- Vite / React / Vue / Svelte SPA：支持 static 输出；
- Next.js：支持 `output = "export"` 或等价 static export / SSG 输出；
- Nuxt：支持 `ssr = false` SPA 或 prerender static 输出；
- SvelteKit：支持 `adapter-static`；
- Astro：支持 `output = "static"`；
- Custom Output：第一阶段先不实现；
- SSR / ISR / Serverless / Edge / Middleware：识别后失败并返回明确错误。

项目部署不直接消费任意框架构建产物，而是统一消费 Grass Output API。构建阶段负责把用户项目输出转换成 `.grass/output`，部署和 serve 阶段只读取 `.grass/output/output.toml`。

Manifest 使用 TOML，而不是 JSON。原因：

- 项目整体配置以 TOML 为主；
- 人类可读性更好；
- 方便用户手写或调试；
- Rust 侧可以直接使用 `toml` + `serde` 解析。

### 15.1 Output 目录结构

第一阶段标准输出目录：

```grass-output-tree.txt
.grass/output/
├── output.toml
├── static/
│   ├── index.html
│   └── assets/
└── metadata/
    ├── build-log.txt
    └── checksums.toml
```

未来可以扩展：

```grass-output-future-tree.txt
.grass/output/
├── output.toml
├── static/
├── server/
│   └── index.js
├── functions/
│   └── api/
└── metadata/
```

### 15.2 `output.toml` v1

最小合法 manifest：

```output.toml
version = 1

[runtime]
kind = "static"

[static]
directory = "static"
```

推荐 static manifest：

```output-static.toml
version = 1

[runtime]
kind = "static"

[framework]
name = "vite"
version = ""

[static]
directory = "static"
spa_fallback = true
not_found = ""

[build]
command = "bun run build"
output_directory = "dist"

[metadata]
generated_by = "grass-node"
generated_at = "2026-01-01T00:00:00Z"
```

未来 SSR manifest 预留：

```output-ssr-future.toml
version = 1

[runtime]
kind = "ssr"

[server]
entry = "server/index.js"
start_command = "node server/index.js"
port_env = "PORT"

[static]
directory = "static"
public_path = "/"
```

### 15.3 Runtime Detection Result

Output API 内部支持以下 runtime kind：

- `static`；
- `ssr`；
- `hybrid`；
- `serverless`；
- `edge`；
- `unsupported`；
- `unknown`。

第一阶段行为：

| runtime kind | 第一阶段行为 |
| --- | --- |
| `static` | 正常部署 |
| `ssr` | deployment failed: `SSR runtime is not implemented yet` |
| `hybrid` | deployment failed: `Hybrid runtime is not implemented yet` |
| `serverless` | deployment failed: `Serverless runtime is not implemented yet` |
| `edge` | deployment failed: `Edge runtime is not implemented yet` |
| `unsupported` | deployment failed，展示 reason |
| `unknown` | 如果 static directory 是有效静态站点，则按 static，否则 failed |

### 15.4 Output Generation 流程

```output-generation-flow.txt
User Source
  ↓
ContainerRuntime build sandbox
  ↓
Build Command
  ↓
Framework Detector
  ↓
Output Adapter / Build Output Inspector
  ↓
.grass/output
  ↓
Artifact Packager
```

如果用户项目已经生成 `.grass/output/output.toml`，第一阶段仍不直接支持 Custom Output。Node 应返回明确错误，后续版本再允许用户自定义 Grass Output：

```custom-output-flow.txt
if .grass/output/output.toml exists:
    fail deployment with "Custom Grass Output is not supported in the first stage"
else:
    detect framework and generate Grass Output
```

### 15.5 Detector、Adapter、Inspector 职责

Detector 负责回答：项目看起来像什么，有哪些信号。

例如：

- `package.json` 包含 `vite`；
- Next.js 配置了 `output: 'export'`；
- Nuxt 配置了 `ssr: false` 或 prerender；
- SvelteKit 使用 `adapter-static`；
- Astro 配置为 `output: 'static'`。

Adapter 负责回答：如何把构建结果转换成 `.grass/output`。

第一阶段需要实现：

- `StaticOutputAdapter`；
- `NextStaticOutputAdapter`；
- `NuxtStaticOutputAdapter`；
- `SvelteKitStaticOutputAdapter`；
- `AstroStaticOutputAdapter`。

后续可以增加：

- `NextOutputAdapter`；
- `NuxtOutputAdapter`；
- `SvelteKitOutputAdapter`；
- `AstroOutputAdapter`；
- `VercelOutputAdapter`；
- `NitroOutputAdapter`。

Inspector 负责检查构建产物：

- 存在 `index.html` 且无 server output -> `static`；
- Next.js static export output -> `static`；
- Nuxt generated static output -> `static`；
- SvelteKit adapter-static output -> `static`；
- Astro static output -> `static`；
- 存在 `.next/server`、`.output/server`、server entry -> `ssr`；
- 存在 `.vercel/output/functions` -> `serverless`；
- 存在 edge function / middleware -> `edge`。

### 15.6 第一阶段部署成功条件

第一阶段部署成功必须满足：

1. `.grass/output/output.toml` 存在；
2. manifest version 支持；
3. `runtime.kind = "static"`；
4. `[static].directory` 存在；
5. static directory 中存在 `index.html`；
6. `.grass/output` 能被安全打包；
7. artifact 上传成功；
8. Node serve 根据 `output.toml` 提供静态服务。

### 15.7 数据模型关联

`deployments` 建议记录：

- `runtime_kind`；
- `output_api_version`；
- `framework_name`；
- `framework_version`；
- `node_id`。

`deployment_artifacts.kind` 建议支持：

- `grass_output`；
- `build_log`。

第一阶段直接打包 `.grass/output` 为 `grass-output.zip`，而不是只打包 `static/`。这样后续接入 SSR / serverless / edge 时 artifact 格式不需要更换。

## 16. 容器化构建与部署运行时

构建和部署都必须通过容器化或隔离运行时抽象执行，第一阶段不能直接把用户命令裸跑在宿主机上。

### 16.1 Runtime 抽象

建议定义 `ContainerRuntime` trait，统一 Podman socket、Docker socket、Apple Container、Jails 等后端。

```container-runtime.rs
pub trait ContainerRuntime {
    async fn prepare_image(
        &self,
        input: PrepareImageInput,
    ) -> Result<PreparedImage, ContainerRuntimeError>;

    async fn run_build(
        &self,
        input: RunBuildInput,
    ) -> Result<BuildExecutionResult, ContainerRuntimeError>;

    async fn run_service(
        &self,
        input: RunServiceInput,
    ) -> Result<RunningService, ContainerRuntimeError>;

    async fn stop_service(
        &self,
        service_id: &str,
    ) -> Result<(), ContainerRuntimeError>;
}
```

后端实现：

- `PodmanSocketRuntime`；
- `DockerSocketRuntime`；
- `AppleContainerRuntime`；
- `JailRuntime`。

说明：Docker 的 DinD、Podman 的 PinP 或类似方案也可以把容器运行时 socket 暴露给外部程序使用，因此 Grass Node 统一按 socket backend 接入，不要求直接访问宿主机 Docker / Podman socket。

### 16.2 第一阶段策略

第一阶段默认 backend 为 `podman-socket`，同时支持 `docker-socket`。

要求：

- build command 在构建容器中执行；
- workspace 通过 bind mount 或 volume 挂载；
- build log 从容器 stdout / stderr 收集；
- `.grass/output` 从容器产物目录读取；
- static serve 可以由 Node 进程读取已解包 artifact；
- 未来 SSR serve 必须通过 `run_service` 启动隔离服务，不直接在宿主机启动用户进程。

### 16.3 配置示例

```node-runtime-config.toml
[runtime]
backend = "podman-socket"
socket = "unix:///run/user/501/podman/podman.sock"
default_build_image = "docker.io/library/node:24-alpine"
network = "bridge"
work_root = "/data/node/workspaces"

[runtime.resources]
cpu_limit = 2
memory_mb = 2048
build_timeout_seconds = 600
```

## 17. 存储设计

第一版使用本地文件系统，后续可以扩展 S3 / MinIO / R2。

建议抽象 `StaticSiteStorage`。

本地路径结构默认以 `/data` 为根：

```storage-layout.txt
/data/
├── deployments/
│   └── <project_id>/
│       └── <deployment_id>/
│           ├── build.log
│           ├── grass-output.zip
│           └── output/
│               ├── output.toml
│               └── static/
└── tmp/
    └── uploads/
```

要求：

- 解压 zip 必须防止路径穿越；
- 读取 build log 必须防止 unsafe path；
- public site path 必须 normalize；
- artifact 上传需要记录 size 和 checksum。

### 17.1 Artifact 清理（TODO）

[TODO] Artifact 清理策略后续作为用户可配置的 Runtime Setting 实现，初步考虑维度包括：

- 构建日志保留天数；
- Preview 部署 artifact 保留天数；
- Production 部署 artifact 保留最近 N 个；
- 失败部署 artifact 保留天数；
- 定时清理任务可配置执行时间；
- 不清理当前 active production deployment 的 artifact。


## 18. 日志与可观测性

要求：

- 使用 `tracing`；
- HTTP 请求使用 `TraceLayer`；
- 日志等级由配置控制；
- 不输出 password、session id、token、cookie、DNS Provider secret；
- 业务错误记录为 `warn`；
- 基础设施错误记录为 `error`；
- deployment 状态变更记录为 `info`；
- quota 拒绝记录为 `warn`。

关键 operation：

- `auth.login`；
- `team.create`；
- `team.invite_member`；
- `team.assign_group`；
- `quota.check`；
- `project.create`；
- `deployment.create`；
- `deployment.claim`；
- `deployment.stage`；
- `artifact.upload`；
- `release.promote`；
- `host.provision`；
- `node.serve.resolve_host`。

## 19. 测试要求

后端必须覆盖：

- slug normalize；
- host normalize；
- path traversal 防护；
- password hash / verify；
- session create / revoke；
- setup mode 判断；
- API error mapping；
- team permission；
- team group quota resolution；
- quota check；
- project permission；
- deployment state transition；
- node claim；
- artifact upload；
- Output API manifest 解析与校验；
- StaticOutputAdapter；
- NextStaticOutputAdapter；
- NuxtStaticOutputAdapter；
- SvelteKitStaticOutputAdapter；
- AstroStaticOutputAdapter；
- ContainerRuntime 抽象 fake backend；
- static site path resolution；
- SPA fallback。

Node 必须覆盖：

- deployment plan build；
- root directory 校验；
- output directory 校验；
- command failure；
- container runtime command failure；
- Grass Output generation；
- archive 打包；
- unsafe path 拒绝；
- serve host resolve；
- serve cache fallback。

前端必须覆盖：

- setup page；
- login page；
- team switcher；
- deployment list；
- deployment detail；
- quota usage display；
- protected route；
- shadcn/ui block smoke test。

质量命令通过 Just 统一执行，不直接要求用户手动组合底层命令。

## 20. 开发命令

根目录通过 Just 统一入口。

建议命令：

```just-commands.txt
just fmt
just clippy
just test
just check
just quality
just run api
just run node
just run console
just install console
just build console
just build api
just build node
just build
```

## 21. 设计约束

- `main.rs` 只做启动编排；
- controller 不直接写复杂业务逻辑；
- feature 负责用例编排；
- domain 不依赖 HTTP；
- infra 不依赖业务 feature；
- Node 不直接访问数据库；
- internal Node API 必须鉴权；
- 对外服务由 Node serve capability 承担；
- public site 访问不能绕过 Host 绑定；
- 所有文件路径操作必须防止路径穿越；
- 所有部署状态转换必须校验前置状态；
- 所有配额消耗必须有事件记录；
- 所有配额拒绝必须返回稳定错误码；
- 所有生产上线必须经过审核策略判断；
- 审核、上线、拒绝、配额拒绝和 Host Provisioning 必须记录审计事件；
- ICP 第一阶段不实现，也不预留字段或接口，仅作为后续合规能力在文档中说明；
- Host provisioning 必须通过 trait 抽象；
- Output API manifest 使用 TOML，文件名为 `.grass/output/output.toml`；
- 构建和部署必须通过 `ContainerRuntime` 抽象，不直接裸跑用户命令；
- 第一阶段至少支持 `podman-socket` 和 `docker-socket` backend，并预留 Apple Container 和 Jail backend；
- SSR runtime kind 必须从第一阶段保留字段和接口；
- 第一阶段 SSR deployment 应明确失败并提示尚未实现；
- DNS Provider secret 不得提交仓库；
- Web Console 必须使用 Vite+，不要按普通 Vite 项目编写工具链文档；
- UI 初始风格使用 shadcn/ui 官方 blocks 和官方默认风格。

## 22. License 与开源合规

本项目基于 BSD 3-Clause License 开源。

### 22.1 根目录 LICENSE

仓库根目录必须包含 `LICENSE` 文件，内容使用标准 BSD 3-Clause License 文本，并填写项目版权持有人和年份。

建议版权声明格式：

```license-copyright.txt
Copyright (c) <year>, <copyright holder>
All rights reserved.
```

如果版权持有人后续需要调整，应同时更新：

- 根目录 `LICENSE`；
- README 中的 License 章节；
- 发布包中的 license metadata；
- 需要显式展示版权信息的二进制、镜像或文档产物。

### 22.2 文件头管理策略

第一阶段不要求每个源代码文件都添加完整 license header，也不要求每个文件复制 BSD 3-Clause 全文。

推荐策略：

- 根目录 `LICENSE` 是项目许可的主声明；
- README 明确写明项目使用 BSD 3-Clause License，并链接到 `LICENSE`；
- Cargo crate、npm package、Docker image、release artifact 等发布产物应携带正确 license metadata；
- 新建源码文件默认不添加文件头，避免样板内容过多；
- 由第三方复制、改写或生成的代码必须保留其原始版权和许可声明；
- 如果某个文件来自外部项目或包含第三方片段，必须在文件头或相邻说明中标明来源、许可和必要版权声明；
- 如后续需要 SPDX 扫描或企业合规分发，可以统一改为简短 SPDX 文件头。

如需启用文件头，推荐使用简短 SPDX 格式，而不是完整 license 文本：

```license-header.txt
// SPDX-License-Identifier: BSD-3-Clause
```

不同文件类型应使用对应注释语法，例如 Rust / TypeScript 使用 `//`，Shell 使用 `#`，HTML 使用 `<!-- -->`。

### 22.3 依赖许可管理

项目依赖必须与 BSD 3-Clause 分发方式兼容。

要求：

- 引入新依赖时优先选择 MIT、Apache-2.0、BSD、ISC 等宽松许可；
- 避免引入 GPL / AGPL 等可能影响整体分发方式的依赖，除非用户明确确认；
- 发布前应生成或检查第三方依赖 license 清单；
- 前端、后端和 Docker 镜像依赖都应纳入 license 检查范围；
- 如果依赖带有 NOTICE 或 attribution 要求，发布产物必须保留对应声明。

建议后续提供统一命令：

```license-commands.txt
just license-check
just license-report
```

### 22.4 CI 检查

CI 后续应加入 license 检查，至少覆盖：

- 根目录存在 `LICENSE`；
- Rust crate metadata 中声明 BSD 3-Clause；
- Console package metadata 中声明 BSD-3-Clause；
- 第三方依赖 license 不包含未批准的强 copyleft 许可；
- 发布产物包含 license 文件。

## 23. CI/CD

项目自身通过 GitHub Actions 完成 CI/CD，使用 Just 统一命令入口。

```yaml
# .github/workflows/ci.yml
- just clippy
- just test
- just console-check
- just console-test

# .github/workflows/build.yml (push main / tag)
- just build-release
- docker build + push
```

要求：

- PR 必须通过 CI（clippy + test + console-check）才能合并；
- main 分支 push 自动构建 Docker image；
- tag push 自动构建 release binary 和 Docker image。
