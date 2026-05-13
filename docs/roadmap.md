# Roadmap

当前版本：0.1.0

本文档基于 `docs/architecture.md` 编写，用于指导 `grass-worker` 第一阶段落地。

第一阶段的核心原则：**尽快完成第一目标全链路可用**。第一目标不是一个极简 demo，而是应用的基础闭环，因此不能为了“最小实现”砍掉 Deployment Page、构建、日志、产物、自动域名、Node serve、团队、权限、配额、审核、审计等基础能力。

## 目标定义

第一阶段完成后，用户应能从零开始完成以下完整流程：

1. 启动 Control API、Node 和 Web Console；
2. 通过 setup flow 初始化系统、管理员、存储、首个 Node、默认团队分组、默认配额计划和 Host Source；
3. 登录 Console，进入个人团队或普通团队；
4. 创建项目并配置源码、构建命令、输出规则和部署环境；
5. 自动分配平台域名或绑定项目 Host；
6. 创建 Production / Preview deployment；
7. Node 领取 deployment，在容器运行时中拉取源码、执行 install/build、生成 Grass Output、打包 artifact；
8. Console 实时查看构建状态和 WebSocket 构建日志；
9. 构建成功后查看 deployment 详情、artifact、来源 revision、访问 URL 和时间线；
10. 根据审核策略完成上线审核；
11. 激活 Production deployment，Node 对外 serve 静态站点；
12. 使用公开 URL 访问 Production / Preview 静态站点；
13. 可以取消、重试、promote、rollback deployment；
14. 团队成员、角色、权限和配额限制生效；
15. 关键行为写入审计事件。

## 第一阶段范围

### 必须完成

- Control API 基础服务；
- Node build + serve 一体节点；
- Web Console；
- setup flow；
- 用户登录与 session；
- 个人团队、普通团队、成员、角色；
- 团队分组；
- 配额计划、配额检查、用量统计；
- 项目管理；
- Deployment 创建、列表、详情；
- 构建状态与上线状态双状态模型；
- 构建日志持久化与 WebSocket 实时日志；
- Grass Output API v1；
- static site artifact 打包、上传、解包和 serve；
- Production / Preview 环境；
- commit / branch / source revision 信息展示；
- cancel / retry / promote / rollback；
- Host Source、Host Binding、Host Provisioning；
- 平台域名自动分配；
- 泛域名解析模式；
- DNS Provider API 模式的抽象与至少一个可用实现或可配置占位实现；
- 上线审核；
- 审计事件；
- SSR 字段、枚举和接口预留；
- SSR / hybrid / serverless / edge 等非 static runtime 的明确失败提示；
- Docker / Podman socket container runtime backend；
- path traversal 防护；
- 基础测试与质量命令。

### 第一阶段不做但需要明确处理

这些内容不实现实际能力，但第一阶段需要给出受控行为：

- SSR deployment：保留字段和接口，检测到后返回未实现错误；
- Custom Grass Output：第一阶段不接受用户自定义 `.grass/output/output.toml`，检测到后返回明确错误；
- 无可用 Host Source 或 Host 配额不足：项目创建不失败，但不自动分配 host，并在 Console 展示原因；
- DNS Provider 临时失败：Host Binding 进入 `pending` 或 `failed`，记录 provision event，并提供重试入口；
- Node capability 被配置为只 build 或只 serve：启动时自动修正为 build + serve 并输出警告。

## Milestone 0：工程骨架与基础设施

目标：搭出可以持续开发、测试和运行的项目骨架。

### 可独立实现的小功能

- **M0.1 Cargo workspace 初始化**：创建 workspace、Control API、Node 和内部 crates 空包，让 Rust 工程可以独立编译；
- **M0.2 后端质量命令**：配置 `rustfmt`、clippy、test 和 Just 命令，不依赖业务代码即可完成；
- **M0.3 Control API 空服务**：实现配置读取、日志初始化和 `/health`，可独立启动；
- **M0.4 Node 空进程**：实现 Node 配置读取、日志初始化和空生命周期，可独立启动；
- **M0.5 Console 工程初始化**：创建 Vite+ React 工程、Tailwind、shadcn/ui 和基础路由，可独立运行；
- **M0.6 Console 质量命令**：接入 `vp check`、`vp test`、`vp build`，可独立验证前端工程；
- **M0.7 统一开发入口**：补齐 Just 命令，把后端、Node、Console 的常用命令统一暴露。

### 后端 Workspace

- 创建 Cargo workspace；
- 创建 `apps/control-api`；
- 创建 `apps/node`；
- 创建内部 crates：`assets`、`config`、`crypto`、`session`、`token`、`archive`、`node-protocol`、`validator`；
- 配置 Rust 2024 edition；
- 配置 workspace dependencies；
- 配置 `rustfmt.toml`；
- 配置 `Justfile`；
- 提供统一命令：`just fmt`、`just clippy`、`just test`、`just check`、`just quality`、`just run-api`、`just run-node`、`just migrate`。

### 前端 Workspace

- 创建 `apps/console`；
- 使用 Vite+、TypeScript、React、React Router、TanStack Query、Tailwind CSS、shadcn/ui；
- 配置 `vite.config.ts`，通过 `vite-plus` 的 `defineConfig` 统一管理；
- 初始化 shadcn/ui 官方默认风格；
- 引入 dashboard 和 login 相关官方 block 作为页面基线；
- 通过 Just 暴露 `console-install`、`console-dev`、`console-check`、`console-test`、`console-build`，内部调用 `vp`。

### 配置与日志

- 实现 Control API bootstrap config；
- 实现 Node bootstrap config；
- 支持 TOML + ENV override；
- 初始化 `tracing`；
- 输出稳定 `operation` 字段；
- 禁止输出 password、session、token、cookie、DNS Provider secret。

### 验收标准

- `just check` 可以运行；
- `just run-api` 能启动空 Control API；
- `just run-node` 能启动空 Node；
- `just console-dev` 能启动 Console；
- `/health` 返回成功。

## Milestone 1：数据库、迁移、Seed 与 Setup Flow

目标：让系统可以从空环境初始化为可登录、可管理、可运行的 ready 状态。

### 可独立实现的小功能

- **M1.1 数据库连接与配置**：接入 PostgreSQL 连接池、配置校验和启动失败处理；
- **M1.2 Migration 基础设施**：接入 SeaORM Migration、自动迁移配置和手动迁移命令；
- **M1.3 核心用户与团队表**：创建 users、credentials、teams、team members 等基础表；
- **M1.4 项目部署相关表**：创建 projects、deployments、artifacts、events、reviews、releases；
- **M1.5 配额与 Host 表**：创建 quota、host source、host binding、provision event 等表；
- **M1.6 Node、审计与设置表**：创建 nodes、audit events、system settings；
- **M1.7 幂等 Seed**：初始化默认团队分组、配额计划、角色、Host Policy 和审核策略；
- **M1.8 Setup API**：实现 setup state、database、admin、site、node、storage、finish 接口；
- **M1.9 Setup Console**：实现 setup 页面和流程状态管理；
- **M1.10 Ready Mode 切换**：setup 完成后从 setup mode 切换到 ready mode。

### 数据库与迁移

- 接入 PostgreSQL；
- 接入 SeaORM；
- 实现 migration 运行入口；
- 支持 `database.auto_migrate`；
- 实现 `gworker migrate` 或等价统一迁移命令；
- migration 失败时服务启动失败并输出明确错误；
- 实现幂等 seed。

### 核心数据表

- `users`；
- `user_password_credentials`；
- `teams`；
- `team_groups`；
- `team_members`；
- `team_invitations`；
- `quota_plans`；
- `quota_limits`；
- `quota_usage_counters`；
- `quota_events`；
- `projects`；
- `deployments`；
- `deployment_events`；
- `deployment_artifacts`；
- `deployment_reviews`；
- `releases`；
- `audit_events`；
- `host_sources`；
- `project_host_bindings`；
- `host_policies`；
- `host_provision_events`；
- `nodes`；
- `system_settings`。

### Setup API

- `GET /api/v1/setup/state`；
- `POST /api/v1/setup/database`；
- `POST /api/v1/setup/admin`；
- `POST /api/v1/setup/site`；
- `POST /api/v1/setup/node`；
- `POST /api/v1/setup/storage`；
- `POST /api/v1/setup/finish`。

### Setup Console

- setup state 页面；
- database 配置页面；
- initial admin 页面；
- site config 页面；
- first node 页面；
- storage root 页面，默认 `/data`；
- finish 页面。

### Seed 内容

- 默认团队分组：`free`、`student`、`plus`、`pro`、`ultra`；
- 默认配额计划；
- 默认角色；
- 默认管理员；
- 默认个人团队；
- 默认 Host Source（如果配置存在）；
- 默认 Host Policy；
- 默认 release review policy。

### 验收标准

- 空数据库启动后进入 setup mode；
- setup 完成后进入 ready mode；
- 管理员可以登录；
- 管理员拥有个人团队；
- seed 重复运行不产生重复数据。

## Milestone 2：认证、团队、权限与基础 Console

目标：形成可登录、多团队、多角色的管理基础。

### 可独立实现的小功能

- **M2.1 密码凭据能力**：实现 Argon2id hash / verify 和相关测试；
- **M2.2 Session 与 Cookie**：实现 Redis session、TTL、滑动刷新和 cookie 写入；
- **M2.3 Auth API**：实现 login、logout、me 和登录限流；
- **M2.4 CSRF 防护**：为 mutation 请求接入 `X-CSRF-Token` 校验；
- **M2.5 Team API 基础 CRUD**：创建、列表、查看、更新团队；
- **M2.6 Team Member API**：成员列表、邀请、改角色、移除成员；
- **M2.7 权限 Guard**：实现 owner/admin/member/viewer 的权限判断；
- **M2.8 Login Console**：实现登录页、登录态恢复和错误展示；
- **M2.9 App Shell 与 Team Switcher**：实现受保护布局、团队切换和基础导航；
- **M2.10 Team Settings 页面**：实现成员和团队基础设置页面。

### Auth

- Argon2id 密码哈希；
- 登录；
- 登出；
- `GET /api/v1/me`；
- Session ID 使用安全随机值；
- Redis session，支持 TTL 和滑动刷新；
- Cookie 使用 `HttpOnly`、`Secure`、`SameSite=Strict`、`Path=/api`；
- mutation 请求要求 `X-CSRF-Token`；
- 登录限流。

### Team

- 创建团队；
- 列出团队；
- 查看团队；
- 更新团队；
- 查看成员；
- 邀请成员；
- 修改成员角色；
- 移除成员；
- 个人团队作为特殊团队处理；
- 权限统一基于团队成员关系判断。

### Role

- `owner`；
- `admin`；
- `member`；
- `viewer`。

### Console

- login 页面；
- protected route；
- app shell；
- team switcher；
- team settings；
- members 页面；
- 基础 admin 页面入口。

### 验收标准

- 未登录访问受保护 API 返回 `401`；
- 非团队成员访问团队资源返回 `403`；
- 不同角色权限符合设计；
- Console 可以登录、切换团队、查看成员。

## Milestone 3：配额系统

目标：第一阶段即实现配额定义、检查、消耗、用量统计和错误展示。

### 可独立实现的小功能

- **M3.1 Quota 数据访问层**：实现 quota plans、limits、usage counters、events 的 domain 查询；
- **M3.2 配额计划 Admin API**：实现配额计划列表、创建、更新接口；
- **M3.3 团队分组到配额解析**：实现 team group 默认 plan 和 explicit plan 覆盖逻辑；
- **M3.4 Redis 原子配额脚本**：实现多维度 Lua check + consume + rollback；
- **M3.5 并发构建信号量**：实现 claim 前获取槽位、完成释放和 TTL 兜底；
- **M3.6 配额事件写入**：业务成功后写入 `quota_events`，支持后续重算；
- **M3.7 用量聚合与校准**：实现 `quota_usage_counters` 聚合和 Redis 计数器重建；
- **M3.8 业务接入点**：接入创建项目、邀请成员、绑定 Host、创建 deployment、上传 artifact 等检查；
- **M3.9 Quota API 与 Console**：实现团队配额、用量页面和超限错误展示；
- **M3.10 Quota 审计**：配额拒绝写入稳定错误码和 audit event。

### Quota Model

- 配额计划管理；
- 配额限制项管理；
- 团队分组绑定默认配额计划；
- 团队可选 explicit quota plan；
- 用量 counter；
- quota events。

### 第一阶段配额维度

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
- 最大 SSR 进程数预留；
- SSR 月运行小时数预留。

### 原子性

- 使用 Redis Lua 完成多维度原子检查和预消耗；
- 任一维度超限时回滚 Redis 计数；
- 业务成功后写入 `quota_events`；
- 业务失败时回滚 Redis；
- 并发构建使用 Redis 信号量模型；
- 并发构建 key 设置 TTL，避免 Node 崩溃永久占用；
- 使用 `quota_usage_counters` 做异步聚合和精确查询；
- 支持从 PostgreSQL 重建 Redis 计数器。

### API 与 Console

- `GET /api/v1/teams/{team_id}/quota`；
- `GET /api/v1/teams/{team_id}/quota/usage`；
- `GET /api/v1/admin/quota-plans`；
- `POST /api/v1/admin/quota-plans`；
- `PATCH /api/v1/admin/quota-plans/{plan_id}`；
- quota usage 页面；
- quota exceeded 错误在 Console 清晰展示。

### 验收标准

- 创建项目时检查项目数配额；
- 邀请成员时检查成员数配额；
- 绑定 Host 时检查 Host 配额；
- 创建 deployment 时检查部署次数配额；
- Node claim 前检查并发构建配额；
- 构建完成后记录构建分钟数；
- 上传 artifact 时检查存储配额；
- 所有配额拒绝返回稳定错误码并记录审计事件。

## Milestone 4：项目与 Host Provisioning

目标：创建项目时形成团队归属、源码配置、平台域名和 Host 绑定基础。

### 可独立实现的小功能

- **M4.1 Project Domain 与 API**：实现项目创建、列表、详情、更新；
- **M4.2 Project 生命周期操作**：实现 archive、unarchive、soft delete、restore、transfer、hard delete；
- **M4.3 Source Config 模型**：保存 Git URL、分支、root、install/build command、output directory；
- **M4.4 Host Source 管理**：实现 Host Source admin CRUD 和默认 source 约束；
- **M4.5 HostProvisioner Trait**：定义 trait、输入输出类型和错误类型；
- **M4.6 Wildcard Provisioner**：实现泛域名模式内部绑定；
- **M4.7 Manual / DNS Provider Provisioner**：实现 manual 模式，并接入 DNS Provider 抽象或可配置占位实现；
- **M4.8 Host Binding API**：实现项目 Host 的列表、创建、删除、更新、primary、provision；
- **M4.9 项目创建自动分配域名**：按规则选择 source、生成 host、处理冲突和失败；
- **M4.10 Project / Host Console**：实现项目列表、详情、设置和 Hosts tab。

### Project

- 创建项目；
- 列出项目；
- 查看项目；
- 更新项目；
- archive / unarchive；
- soft delete / restore；
- transfer team；
- hard delete；
- 项目必须归属 `team_id`；
- 项目包含 `runtime_kind`，第一阶段支持 `static` 和 `ssr` 枚举；
- SSR 项目允许创建，但实际 deployment 明确失败。

### Source Config

- Git 仓库 URL；
- 分支；
- root directory；
- install command；
- build command；
- output directory；
- framework hint 可选；
- Production / Preview 环境配置。

### Host Source

- 管理 Host Source；
- 支持 `wildcard`；
- 支持 `dns_provider` 抽象；
- 支持 `manual`；
- 字段包含 `kind`、`label`、`base_domain`、`enabled`、`allows_auto_assign`、`is_default`、`provider`、`config`。

### Host Binding

- 项目 Host 列表；
- 创建 Host；
- 删除 Host；
- 修改 Host；
- 设置 primary Host；
- 触发 provision；
- provision events 落库；
- 自动分配平台域名。

### Host Provisioning 行为

- 创建项目时尝试自动分配平台域名；
- Host 配额不足或无可用 source 时项目创建成功，但不自动分配 host；
- DNS Provider 失败时 Host Binding 进入 `pending` 或 `failed`；
- Console 展示错误和重试入口；
- HostProvisioner 必须通过 trait 抽象；
- `WildcardHostProvisioner` 创建内部绑定；
- `ManualHostProvisioner` 生成待配置状态；
- `CompositeHostProvisioner` 根据 Host Source 类型分发。

### 验收标准

- 创建项目后能在 Console 看到项目详情；
- 如果存在唯一可自动分配 Host Source，项目自动获得平台域名；
- Host 冲突会被拒绝；
- Host 配额生效；
- Host provision 成功、pending、failed 都有事件记录。

## Milestone 5：Deployment Control Plane

目标：完成部署元数据、状态流转、审核、审计和 API。

### 可独立实现的小功能

- **M5.1 Deployment Domain**：实现 deployments、events、artifacts、reviews、releases 的领域模型；
- **M5.2 状态机校验**：实现 build status 和 release status 的合法流转检查；
- **M5.3 创建 Deployment**：实现创建接口、初始状态、来源 revision 字段和审计；
- **M5.4 Deployment 查询 API**：实现列表、详情、events、timeline、artifacts 查询；
- **M5.5 Cancel 操作**：实现 cancel API、状态转换和 Node 后续协作标记；
- **M5.6 Retry 操作**：实现从失败或取消部署创建重试部署；
- **M5.7 Promote 操作**：实现 ready deployment 的上线前置检查和状态变更；
- **M5.8 Review API**：实现 request、approve、reject 和审核记录；
- **M5.9 Rollback 语义**：实现指向旧 deployment 的 release 记录和 active 切换；
- **M5.10 Deployment 页面数据契约**：为列表和详情准备完整 response DTO。

### Deployment Model

- `deployments` 记录构建状态和上线状态；
- `deployment_events` 记录状态时间线；
- `deployment_artifacts` 记录 artifact；
- `deployment_reviews` 记录审核；
- `releases` 记录上线时间线；
- `audit_events` 记录关键行为。

### Build Status

- `pending`；
- `claimed`；
- `queued`；
- `building`；
- `ready`；
- `failed`；
- `canceled`。

### Release Status

- `draft`；
- `pending_review`；
- `approved`；
- `rejected`；
- `active`。

### Deployment API

- 创建 deployment；
- deployment 列表；
- deployment 详情；
- cancel；
- retry；
- promote；
- build log 查询；
- artifacts 查询；
- events 查询；
- timeline 查询。

### Review API

- request review；
- approve；
- reject；
- Production promote / activate 必须经过审核策略判断；
- Preview 是否需要审核由项目或系统策略决定。

### 状态约束

- 只有 `build_status = ready` 才能进入 release 流程；
- 同一项目同一环境只能有一个 `active` deployment；
- promote 时目标 deployment 变为 `active`，旧 active 变为 `approved`；
- rollback 时创建新的 release 记录指向旧 deployment，并让旧 deployment 重新成为 `active`；
- 所有状态转换必须校验前置状态；
- 所有关键状态转换记录 deployment event 和 audit event。

### 验收标准

- Console 可以创建 deployment；
- deployment 初始进入 `pending`；
- deployment 列表展示状态、环境、分支、commit、触发人、项目、创建时间、构建耗时、阶段、失败摘要、访问 URL 和操作入口；
- deployment 详情展示总览、时间线、日志、阶段、源码、artifact、环境、访问地址、域名状态、错误详情和操作按钮；
- 不合法状态转换被拒绝并返回稳定错误。

## Milestone 6：Internal Node API 与 Node 注册心跳

目标：让 Node 通过内部协议参与构建和 serve，不直接访问数据库。

### 可独立实现的小功能

- **M6.1 Node Token 生成与存储**：创建 Node 时生成 token，数据库只存 SHA-256 hash；
- **M6.2 Internal Auth Middleware**：实现 Bearer token 校验、constant-time compare 和吊销检查；
- **M6.3 Node Register API**：实现 Node 注册、capabilities 上报和版本记录；
- **M6.4 Heartbeat API**：实现心跳更新和 unhealthy 标记任务；
- **M6.5 Node Client**：Node 侧封装 Control API client、认证 header 和错误处理；
- **M6.6 Claim API**：实现 deployment claim、Redis lock、node_id 写入；
- **M6.7 Stage API**：实现 Node 回报阶段和状态变更；
- **M6.8 Build Log / Static Site Internal API**：实现日志写入和 artifact 上传入口；
- **M6.9 Serve Resolve API**：实现 Host 到 active / preview deployment 的解析接口；
- **M6.10 Admin Nodes Console**：展示 Node 列表、健康状态和基础详情。

### Node Auth

- 每个 Node 独立 token；
- token 明文只在创建时展示一次；
- 数据库存 SHA-256 hash；
- 使用 constant-time compare；
- 支持 token 吊销；
- Redis 黑名单缓存即时生效。

### Internal API

- `POST /api/v1/internal/nodes/register`；
- `POST /api/v1/internal/nodes/heartbeat`；
- `POST /api/v1/internal/deployments/claim`；
- `POST /api/v1/internal/deployments/{deployment_id}/stage`；
- `PUT /api/v1/internal/deployments/{deployment_id}/build-log`；
- `PUT /api/v1/internal/deployments/{deployment_id}/static-site`；
- `GET /api/v1/internal/serve/resolve-host`。

### Node Runtime

- Node 启动读取配置；
- 校验 control API URL、node token、capabilities；
- 注册 Node；
- 每 30 秒 heartbeat；
- API 标记超过 90 秒无心跳的 Node 为 unhealthy；
- 第一阶段 Node 必须 build + serve；
- 如果配置关闭某个 capability，启动时自动修正并警告。

### Claim 行为

- Node 轮询 claim deployment；
- claim 使用 Redis lock 或等价原子机制；
- claim 前检查并发构建配额；
- claim 成功后写入 `deployments.node_id`；
- claim 失败不重复构建同一个 deployment。

### 验收标准

- Node 注册后 Console admin nodes 页面可见；
- heartbeat 正常更新健康状态；
- token 错误的 Node 无法访问 internal API；
- 单个 deployment 不会被多个 Node 同时 claim。

## Milestone 7：Container Runtime 与 Build Pipeline

目标：让 Node 能在隔离运行时中完成真实静态站点构建。

### 可独立实现的小功能

- **M7.1 ContainerRuntime Trait**：定义 prepare image、run build、run service、stop service 接口；
- **M7.2 Docker Socket Backend**：实现 Docker socket 的 build command 执行；
- **M7.3 Podman Socket Backend**：实现 Podman socket 的 build command 执行；
- **M7.4 Runtime 配置与资源限制**：支持 image、socket、network、CPU、memory、timeout；
- **M7.5 Git Checkout**：实现仓库拉取、branch / commit checkout 和 root directory 校验；
- **M7.6 Build Command Runner**：在容器中执行 install/build，并收集 stdout/stderr；
- **M7.7 Build Stage Reporter**：向 Control API 回报 queued、building、archive、upload、ready/failed；
- **M7.8 Cancel / Timeout 处理**：停止容器、标记状态并释放并发构建 quota；
- **M7.9 Artifact 上传接入**：打包完成后上传 artifact metadata 和文件；
- **M7.10 Build Failure 归因**：记录失败阶段、错误摘要和可展示 message。

### ContainerRuntime

- 定义 `ContainerRuntime` trait；
- 实现 `podman-socket` backend；
- 实现 `docker-socket` backend；
- 保留 Apple Container 和 Jail backend 类型，但不要求实际可用；
- 支持 prepare image；
- 支持 run build；
- 支持 run service 接口预留；
- 支持 stop service 接口预留；
- 配置默认 build image；
- 配置 CPU、memory、timeout、network、socket。

### Build Pipeline

- 拉取 Git 仓库；
- 校验 root directory；
- 在容器中执行 install command；
- 在容器中执行 build command；
- 收集 stdout / stderr；
- 写入 build log；
- 推送实时 log frame；
- framework detect；
- output adapter 转换 `.grass/output`；
- 校验 output manifest；
- 打包 `grass-output.zip`；
- 计算 checksum 和 size；
- 上传到本地 storage；
- 回报 status 和 artifacts。

### 失败处理

- Git clone 失败进入 `failed`；
- install 失败进入 `failed`；
- build 失败进入 `failed`；
- runtime kind 不支持进入 `failed` 并展示明确原因；
- timeout 进入 `failed` 或 `canceled`，按实际触发源区分；
- cancel 时停止容器构建并释放并发构建 quota；
- Node 崩溃后由 TTL 兜底释放并发槽位。

### 验收标准

- 一个 Vite static 项目可以从 Git 仓库构建成功；
- 构建日志可在构建期间持续产生；
- 构建失败有清晰失败阶段和错误摘要；
- artifact 信息落库；
- quota build minutes 和 artifact storage usage 被记录。

## Milestone 8：Grass Output API 与 Static Artifact

目标：统一构建产物格式，让 serve 阶段只消费 `.grass/output`。

### 可独立实现的小功能

- **M8.1 Manifest 类型与解析**：实现 `output.toml` v1 的 serde 类型、解析和错误；
- **M8.2 Manifest 校验**：校验 version、runtime kind、static directory 和 `index.html`；
- **M8.3 Package / Framework Detector**：从 package.json 和配置文件识别 Vite、Next、Nuxt、SvelteKit、Astro；
- **M8.4 StaticOutputAdapter**：处理通用静态输出目录；
- **M8.5 Next Static Adapter**：处理 Next.js static export；
- **M8.6 Nuxt Static Adapter**：处理 Nuxt SPA / prerender static；
- **M8.7 SvelteKit / Astro Adapter**：处理 adapter-static 和 Astro static output；
- **M8.8 Unsupported Runtime Inspector**：识别 SSR、hybrid、serverless、edge 并返回明确失败；
- **M8.9 Archive 模块**：安全打包 `.grass/output`，计算 size 和 checksum；
- **M8.10 Artifact 解包与路径防护**：实现 zip 解包防穿越和 artifact 读取防 unsafe path。

### Manifest

- `output.toml` 使用 TOML；
- 支持 `version = 1`；
- 支持 `[runtime].kind = "static"`；
- 支持 `[static].directory`；
- 支持 `spa_fallback`；
- 支持 framework metadata；
- 支持 build metadata；
- 校验 manifest version；
- 校验 static directory；
- 校验 `index.html`。

### Detector / Adapter / Inspector

- Package JSON detector；
- Vite detector；
- Next static export detector；
- Nuxt static detector；
- SvelteKit adapter-static detector；
- Astro static detector；
- StaticOutputAdapter；
- NextStaticOutputAdapter；
- NuxtStaticOutputAdapter；
- SvelteKitStaticOutputAdapter；
- AstroStaticOutputAdapter；
- SSR / hybrid / serverless / edge inspector。

### Artifact

- 打包整个 `.grass/output` 为 `grass-output.zip`；
- 同时保存 build log；
- zip 解包防路径穿越；
- 读取 artifact 防 unsafe path；
- 记录 checksum；
- 记录 artifact size；
- 记录 `runtime_kind`、`output_api_version`、`framework_name`、`framework_version`。

### 验收标准

- Vite / React / Vue / Svelte SPA static 输出可部署；
- Next.js static export 可部署；
- Nuxt static / SPA 输出可部署；
- SvelteKit adapter-static 输出可部署；
- Astro static 输出可部署；
- SSR / serverless / edge 输出会失败并提示尚未实现；
- 自定义 `.grass/output/output.toml` 会失败并提示第一阶段不支持 Custom Output。

## Milestone 9：Realtime Build Logs

目标：Node、Control API、Browser 之间形成实时日志闭环。

### 可独立实现的小功能

- **M9.1 日志消息协议类型**：定义 log、stage_change、done、subscribe、cancel DTO；
- **M9.2 Node 日志采集器**：从容器 stdout/stderr 行缓冲读取并分配 seq；
- **M9.3 Node 日志持久化**：边构建边写 `build-log.txt`；
- **M9.4 Node → API WS 通道**：实现 Node 到 Control API 的日志推送；
- **M9.5 API 日志中继 Hub**：按 deployment 管理订阅者并广播日志；
- **M9.6 Browser WS Endpoint**：实现 subscribe、cancel 和权限校验；
- **M9.7 HTTP 补拉接口**：实现 `build-log?after_seq=...`；
- **M9.8 Console 日志 Viewer**：实时展示、自动滚动、暂停滚动、重连状态；
- **M9.9 Cancel 上行链路**：Browser 发送 cancel 后传递到 Control API 和 Node；
- **M9.10 日志流审计**：记录 log stream started / ended 事件。

### 协议

- JSON over WebSocket；
- Node → Control API；
- Control API → Browser；
- 支持 `log`；
- 支持 `stage_change`；
- 支持 `done`；
- Browser 上行支持 `subscribe`；
- Browser 上行支持 `cancel`。

### 持久化与重连

- Node 边构建边写 `build-log.txt`；
- Node 边构建边推 WS frame；
- 每行日志包含 `seq`；
- HTTP `GET build-log?after_seq=...` 支持补拉；
- Browser 维护 `last_seq`；
- 断线重连后先补拉再继续订阅；
- 构建完成后完整日志作为 artifact 保存。

### Console

- deployment detail 中展示实时日志；
- 支持阶段分组；
- 支持自动滚动；
- 支持暂停滚动；
- 支持重连状态提示；
- 支持 cancel 操作。

### 验收标准

- 构建期间 Console 能看到实时日志；
- 刷新页面后可以看到历史日志并继续接收新日志；
- 断线重连不丢日志；
- cancel 能从 Console 发出并停止构建。

## Milestone 10：Node Serve 与公开访问 URL

目标：构建成功并激活后，公开 URL 可以访问静态站点。

### 可独立实现的小功能

- **M10.1 Public Serve HTTP Server**：Node 启动对外 HTTP server；
- **M10.2 Host Resolve Client 与 Cache**：调用 internal resolve-host 并缓存结果；
- **M10.3 Production Active 解析**：稳定域名只解析到当前 active production deployment；
- **M10.4 Preview URL 解析**：预览域名解析到指定 preview deployment；
- **M10.5 Artifact Local Resolver**：定位本地 artifact 和解包缓存；
- **M10.6 Static File Resolver**：按 `output.toml` 解析 static directory、index 和 SPA fallback；
- **M10.7 HTTP Response 细节**：设置 MIME、cache-control、404 和错误页；
- **M10.8 Path Traversal 防护**：统一 normalize public path，拒绝 unsafe path；
- **M10.9 URL 展示字段**：Control API 为 deployment 列表和详情返回 production / preview URL；
- **M10.10 Serve 测试样例**：覆盖 index、SPA fallback、非法路径和缓存刷新。

### Resolve Host

- Node 接收 public HTTP request；
- 根据 Host 调用或缓存 Control API resolve-host 结果；
- 区分 Production active deployment 和 Preview deployment；
- 未绑定 Host 返回受控错误页面或 `404`；
- 非 active Production 不对生产稳定域名 serve。

### Static Serve

- 定位 deployment artifact；
- 解包或读取缓存；
- 根据 `output.toml` serve static directory；
- 支持 directory `index.html`；
- 支持 SPA fallback；
- 设置 `cache-control`；
- MIME type 使用 `mime_guess`；
- 所有路径必须 normalize，防 path traversal。

### URL

- Production deployment 有稳定 URL；
- Preview deployment 有唯一预览 URL；
- deployment 列表展示访问 URL；
- deployment 详情展示访问 URL；
- Host 绑定状态影响 URL 展示。

### 验收标准

- Production deployment 审核并激活后可通过稳定域名访问；
- Preview deployment 可通过唯一预览域名访问；
- SPA 路由刷新可返回 `index.html`；
- 尝试访问 `../` 等非法路径被拒绝；
- Host resolve 缓存过期后能刷新。

## Milestone 11：Deployment Page 完整体验

目标：实现类似 Vercel Deployments Page 的核心产品体验。

### 可独立实现的小功能

- **M11.1 Deployment List API DTO**：补齐列表页所需状态、环境、来源、耗时、阶段、URL 和操作权限字段；
- **M11.2 Deployment Detail API DTO**：补齐详情页总览、timeline、logs、artifact、host、error 和操作字段；
- **M11.3 Deployment List UI**：实现列表、状态徽章、环境筛选、操作入口；
- **M11.4 Deployment Detail Overview UI**：实现总览、源码信息、环境和访问地址；
- **M11.5 Timeline UI**：展示 build / release / review / host provision 事件；
- **M11.6 Log Viewer UI 集成**：把实时日志 Viewer 接入详情页；
- **M11.7 Artifact 与错误详情 UI**：展示 artifact metadata、失败阶段、错误摘要；
- **M11.8 Deployment 操作 UI**：实现 cancel、retry、promote、rollback 按钮与状态禁用；
- **M11.9 Project 页面整合**：项目详情、deployment tab、hosts tab、settings tab 串联；
- **M11.10 Admin / Team 页面整合**：quota、members、team groups、host sources、nodes、audit events 页面可访问。

### Deployment List

列表展示：

- 部署状态；
- Production / Preview；
- 来源分支；
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

### Deployment Detail

详情展示：

- 总览；
- 状态时间线；
- 构建日志；
- 构建阶段；
- 源码信息；
- artifact 信息；
- 环境信息；
- 访问地址；
- 域名绑定状态；
- 错误详情；
- retry；
- cancel；
- promote；
- rollback。

### Project Pages

- project list；
- project detail；
- project settings；
- deployment tab；
- hosts tab；
- quota 提示；
- source config 展示与编辑。

### Admin / Team Pages

- quota usage；
- team members；
- team groups；
- quota plans；
- host sources；
- nodes health；
- audit events。

### 验收标准

- 用户可以从 Console 完成创建项目、创建 deployment、看日志、审核、上线、访问站点、回滚的完整流程；
- deployment 列表和详情足以定位失败原因；
- 配额错误、Host provision 错误、Node 错误都有清晰展示。

## Milestone 12：上线审核、审计与操作闭环

目标：上线操作有治理能力，关键行为可追踪。

### 可独立实现的小功能

- **M12.1 Review Policy 模型**：实现系统、团队或项目级审核策略读取；
- **M12.2 Review Request 流程**：构建成功后提交审核并记录状态；
- **M12.3 Approve / Reject 流程**：实现审核通过、驳回和重新提交前置状态；
- **M12.4 Production Activate Gate**：Production active 必须通过审核策略；
- **M12.5 Preview Review 策略**：Preview 是否需要审核由配置决定；
- **M12.6 Release Timeline 写入**：promote、rollback、auto activate 都写入 `releases`；
- **M12.7 Active 唯一性事务**：同一项目同一环境 active 切换在事务中完成；
- **M12.8 Rollback 完整闭环**：从 UI 到 API 完成旧 deployment 重新 active；
- **M12.9 Audit Writer**：统一封装 actor、action、target、result、reason 写入；
- **M12.10 Audit Console**：实现团队和管理员审计事件查询页面。

### Review

- 构建成功后可进入待审核；
- Production promote / activate 必须经过审核策略；
- Preview 可按项目配置决定是否审核；
- approve 后允许 active；
- reject 后回到可重新提交状态；
- 审核记录包含审核人、结论、时间、原因。

### Release

- release timeline 记录 deployment 何时成为 active；
- promote 记录 release；
- rollback 记录 release；
- auto activate 记录 release；
- 同一项目同一环境保证只有一个 active。

### Audit Events

至少记录：

- `deployment.created`；
- `deployment.build_started`；
- `deployment.build_finished`；
- `deployment.review_requested`；
- `deployment.review_approved`；
- `deployment.review_rejected`；
- `deployment.promoted`；
- `deployment.rolled_back`；
- `host.provisioned`；
- `quota.denied`；
- `node.registered`；
- `node.token_revoked`。

### 验收标准

- Production 未审核不能 active；
- 审核通过后可以 promote；
- rollback 后旧 deployment 重新成为 active；
- audit 页面可以查看关键事件；
- 审计事件包含 actor、action、target、timestamp、result、reason。

## Milestone 13：测试、质量、安全与发布

目标：第一阶段达到可自托管试用的质量标准。

### 可独立实现的小功能

- **M13.1 后端领域单元测试**：覆盖 slug、host、权限、状态机、quota resolution；
- **M13.2 后端 API 集成测试**：覆盖 setup、auth、team、project、deployment、host、quota；
- **M13.3 Node 构建测试**：覆盖 checkout、command failure、runtime failure、Grass Output generation；
- **M13.4 Node Serve 测试**：覆盖 host resolve、static path、SPA fallback、unsafe path；
- **M13.5 前端组件与页面测试**：覆盖 setup、login、team switcher、deployment list/detail、quota usage；
- **M13.6 安全测试**：覆盖 path traversal、token 校验、CSRF、错误脱敏；
- **M13.7 CI Workflow**：配置 GitHub Actions 运行 clippy、test、console-check、console-test；
- **M13.8 Docker 构建**：实现多阶段镜像、非 root 用户和必要 runtime 依赖；
- **M13.9 Release 构建**：tag 构建 release binary 和 Docker image；
- **M13.10 自托管文档与 Demo 脚本**：提供 README / 部署文档，确保第一目标全链路可复现。

### 后端测试

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
- ContainerRuntime fake backend；
- static site path resolution；
- SPA fallback。

### Node 测试

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

### 前端测试

- setup page；
- login page；
- team switcher；
- deployment list；
- deployment detail；
- quota usage display；
- protected route；
- shadcn/ui block smoke test。

### 安全检查

- Node internal API 必须鉴权；
- public site 访问不能绕过 Host 绑定；
- 所有文件路径操作防路径穿越；
- DNS Provider secret 不提交仓库；
- cookie 和 CSRF 设置符合设计；
- 错误响应不泄露数据库、Redis、DNS Provider 底层细节。

### 发布

- Docker 多阶段构建；
- runtime 镜像只包含必要二进制和运行依赖；
- 非 root 用户运行；
- GitHub Actions 跑 `just clippy`、`just test`、`just console-check`、`just console-test`；
- main push 构建 Docker image；
- tag push 构建 release binary 和 Docker image。

### 验收标准

- `just quality` 通过；
- Docker image 可启动 Control API 和 Node；
- README 或部署文档可以指导用户完成本地自托管部署；
- 第一目标全链路 demo 可稳定跑通。

## 第一阶段最终验收清单

- [ ] 可以从空数据库完成 setup；
- [ ] 管理员可以登录 Console；
- [ ] 个人团队自动创建；
- [ ] 普通团队、成员、角色可管理；
- [ ] 团队分组和配额计划生效；
- [ ] 项目可创建并自动分配平台域名；
- [ ] Host Source 和 Host Binding 可管理；
- [ ] 可以创建 Production deployment；
- [ ] 可以创建 Preview deployment；
- [ ] Node 可以注册、心跳、claim deployment；
- [ ] 构建在 Podman 或 Docker socket runtime 中执行；
- [ ] 构建日志可以实时查看；
- [ ] 构建日志可以历史补拉；
- [ ] Grass Output v1 static manifest 可生成和校验；
- [ ] static artifact 可打包、上传、解包和 serve；
- [ ] Production deployment 审核通过后可激活；
- [ ] Preview deployment 有唯一访问 URL；
- [ ] Production deployment 有稳定访问 URL；
- [ ] cancel / retry / promote / rollback 可用；
- [ ] deployment 列表页信息完整；
- [ ] deployment 详情页信息完整；
- [ ] 配额拒绝有稳定错误码和 Console 展示；
- [ ] Host provision 失败可见且可重试；
- [ ] 审计事件可查询；
- [ ] SSR / serverless / edge 等 runtime 有明确未实现错误；
- [ ] `just quality` 通过；
- [ ] Docker image 可用于自托管试用。

# Future

以下内容不属于第一阶段，不应混入当前阶段任务：

- 在线支付；
- 发票；
- 订阅扣款；
- OAuth 登录；
- GitHub App 深度集成；
- Serverless Functions；
- Edge Runtime；
- SSR runtime 实际运行与进程管理；
- ISR；
- Middleware；
- 分布式构建缓存；
- build node 和 serve node 分离调度；
- artifact 跨节点同步；
- 多节点负载均衡和 failover；
- S3 / MinIO / R2 artifact storage；
- Apple Container backend 实际实现；
- Jail backend 实际实现；
- Vercel Output API 深度兼容；
- Custom Grass Output；
- 更细粒度的 preview / production 默认 Host Source；
- artifact retention policy 完整配置；
- SMTP / 邮件通知；
- Webhook；
- 中国境内备案访问控制；
- 备案套餐或备案配额自动放行；
- 备案状态、访问控制和停止页面。
