# Self-Hosted Static Platform Master Plan

## 1. Goal

构建一个类似 Vercel 的自部署静态网页部署平台，优先支持单机可跑、再自然扩展到多节点模式。平台首期只聚焦“静态站点构建与发布”，不在早期混入函数计算、数据库托管或对象存储托管。

核心目标：

- 用户或团队可以管理自己的项目、部署记录、域名与配额。
- 平台可以从 Git 源触发构建，并把产物发布到可访问的静态站点。
- 平台既支持单服务器，也支持控制面 + 构建/部署节点的多服务器拓扑。
- 前后端分离，API 与控制台独立开发。
- 默认依赖内部测试桩与本地 mock，不依赖外部在线服务完成测试。

## 2. Architecture Principles

### 2.1 Repository And Runtime Shape

- 使用 Rust workspace 管理多应用与共享 crate。
- 应用拆分保持极简：`app/api`、`app/frontend`、`app/node`。
- `app/api` 负责控制面 API、静态资源出口、未来管理后台入口。
- `app/node` 负责节点注册、任务执行、构建/部署能力上报。
- `app/frontend` 负责控制台前端。

### 2.2 Backend Boundaries

- Axum 只承载 HTTP 适配层。
- 核心业务按 VSA 思路组织：`view` 处理接口输入输出，`service` 处理用例与流程，`adapter` 处理数据库、容器运行时、Git provider、节点通信等外部依赖。
- 领域规则放在 service/domain 一侧，不把配额、权限、订阅规则直接写进 handler。
- `app/api` 不在后端直接拼管理界面 HTML；setup、控制台、管理后台都通过 API + 独立前端完成。

### 2.3 Frontend Delivery Rules

- 发行模式下由 `app/frontend` 构建静态文件到 `crates/assets/assets/public/`，再由 `crates/assets` 嵌入并供 `app/api` 消费。
- 运行时若工作目录存在 `./public/`，优先使用该目录作为前端资源来源。
- 配置文件存在 `[development]` 时，`app/api` 前端路由代理到 `development.dev_server`。
- 开发模式不做静默降级：dev server 不可达时直接报错，避免误把测试流量打到旧静态文件。

### 2.4 Configuration Rules

- 统一使用 TOML 配置。
- `config.toml` 只保留进程启动前必须知道的 boot config，不把所有站点运行时配置都堆进文件里。
- `config.toml` 中保留的典型内容：`[server]`、`[node]` 的监听地址，以及 PostgreSQL 连接配置。
- `[server]` 与 `[node]` 相互独立；一份配置可以只包含其中一个，也可以同时包含两者，不要求所有部署都配置两段。
- `app/api` 与 `app/node` 只能依赖各自启动所需的 boot config；监听地址这类启动前必需信息不能依赖数据库内配置反推。
- 当 `./config.toml` 缺失，或存在 boot config 但缺少数据库配置时，`app/api` 不直接退出，而是以内置临时地址启动首次配置页，用于完成安装引导。
- 首次配置流程采用分阶段 setup：
  - Stage 1: Database。通过 setup API 收集 PostgreSQL `host`、`port`、`db_name`、`user`、`password`、可选 `schema`，写回 `config.toml`，并验证连接。
  - Stage 2: Admin。数据库可用但系统内还没有管理员时，继续停留在 setup 流程，创建首个管理员。
  - Stage 3: Site Settings。未来若确实存在“只需初始化一次”的站点级设置，可以继续追加 stage；这类配置优先进入数据库，而不是继续膨胀 `config.toml`。
- 数据库当前只支持 PostgreSQL，不提前为 MySQL、SQLite 等其他后端做兼容抽象。
- PostgreSQL 配置采用结构化字段，而不是单个连接串字段：`host`、`port`、`db_name`、`user`、`password`，可选 `schema`。
- `schema` 表示 PostgreSQL 内的命名空间，默认使用 `public`；它不是独立数据库，也不替代 `db_name`。
- 控制面启动时自动准备目标 `schema` 并执行待应用迁移；当前不提供单独的 migration CLI 作为主流程。
- 除 boot config 外，其余业务配置优先存数据库，并通过 setup/admin UI 管理。
- `[development]` 仅用于本地开发联调，不与生产配置混用。
- 多节点、容器运行时、反向代理、配额策略都走显式配置，不靠环境推断。

### 2.5 Testing Rules

- 单元测试优先验证纯逻辑：配额计算、权限判定、部署状态机、节点调度策略。
- 集成测试只依赖仓库内部 mock、临时目录、内存服务、本地测试 server。
- 不允许测试依赖真实 GitHub、Gitea、Forgejo、PostgreSQL SaaS、真实 webhook 回调。
- 外部适配器必须抽象接口，以便 mock 或 fake 实现。

### 2.6 Delivery Rules

- 每轮会话只做一个最小功能。
- 每个最小功能都要包含：设计边界、实现、测试、文档更新。
- 计划文档统一存放在 `docs/plans/`。

## 3. Target System Overview

### 3.1 Control Plane

职责：

- 用户认证、权限判定、团队与分组管理
- 项目、部署、域名、订阅、配额管理
- 构建任务与部署任务编排
- 节点注册、心跳、能力管理
- webhook 接收与事件入队

### 3.2 Node Plane

职责：

- 执行构建任务
- 执行部署任务
- 回传日志、状态、产物元数据
- 上报机器标签、角色、资源占用

节点角色：

- `build`
- `deploy`
- `build_deploy`

### 3.3 Delivery Plane

职责：

- 静态文件发布目录管理
- 域名绑定
- 反向代理配置下发或生成
- 流量/带宽/访问量统计接口预留

首期建议：

- 先把“发布目录 + Caddy 配置生成/重载抽象”做出来。
- 真正替换其他代理方案时只替换 adapter。

### 3.4 Source Integration Plane

职责：

- 拉取 Git 仓库
- 处理 GitHub / Gitea / Forgejo webhook
- 识别分支、提交、触发策略
- 管理仓库凭据和 webhook 密钥

### 3.5 Bootstrap And Setup Flow

职责：

- 判断当前实例应进入正常运行模式还是 setup 模式
- 在缺少数据库配置时提供 Stage 1 setup API
- 在数据库已经可用但不存在管理员时提供 Stage 2 首个管理员 setup API
- 为未来站点级初始化设置预留 Stage 3，而不是把全部运行期配置重新塞回文件
- setup 阶段由后端暴露 JSON API；状态探针通过 `/api/v1/info` 获取，数据库/管理员配置分别走 `/api/v1/setup/*`

## 4. Phase Roadmap

## Phase 0: Foundation And Static Asset Chain

目标：先把仓库、基础应用、配置、前端资源链路稳定下来，形成能运行、能测试、能扩展的底座。

范围：

- Rust workspace 与三应用目录结构
- `app/api` / `app/node` Hello World + `/health`
- Bun 前端占位页
- TOML boot config 加载与 `config.toml` 启动引导
- 缺省 boot config 时进入 setup mode 的基础入口与临时监听地址策略
- `[development].dev_server` 代理链路
- `./public` 优先 + `crates/assets` 嵌入回退
- `build.rs` 对嵌入资源做编译期校验
- `just` 命令统一开发流程

完成标准：

- 单机启动后可以访问 API 与前端占位页；首次启动缺少配置时也有明确 setup 入口
- 本地开发可以让 API 代理前端 dev server
- release 构建产物包含前端资源

## Phase 1: Database And Bootstrap Foundation

目标：先把 PostgreSQL 连接、迁移、首次数据库引导、核心表结构和 repository 边界稳定下来，作为后续身份、项目、部署等业务阶段的共用底座。

范围：

- SeaORM 迁移基础设施
- PostgreSQL 连接、schema 选择与 repository 抽象
- PostgreSQL 配置字段采用 `host`、`port`、`db_name`、`user`、`password`，可选 `schema`
- `app/api` 启动时自动准备 `schema` 并执行迁移，不依赖单独 migration CLI
- 首次启动 Stage 1：数据库 setup API，负责收集 PostgreSQL 连接信息、写回 `config.toml`、验证连接并推进迁移
- `projects`、`deployments`、`deployment_artifacts` 基础表
- `projects`
  - `id`、`owner_user_id`、`slug`、`name`、`status`、`created_at`、`updated_at`、`archived_at`
- `deployments`
  - `id`、`project_id`、`status`、`source_branch`、`source_revision`、`created_at`、`started_at`、`finished_at`
- `deployment_artifacts`
  - `id`、`deployment_id`、`kind`、`storage_path`、`checksum_sha256`、`size_bytes`、`created_at`
- `users` 基础表
  - `id`、`email`、`is_admin`、`is_initial_admin`、`created_at`、`updated_at`
- `user_password_credentials`
  - `user_id`、`password_hash`、`password_updated_at`
- `user_sessions`
  - `id`、`user_id`、`token_hash`、`created_at`、`expires_at`、`revoked_at`
- 项目状态与部署状态枚举
- 基础 repository abstraction
- 本阶段只完成数据库结构和持久化边界；不急着把注册、登录、项目 API 一次性混进来

完成标准：

- 缺少数据库配置时，`app/api` 可以进入 Stage 1 setup 并完成数据库初始化
- 可以在 `app/api` 启动时连接到指定 PostgreSQL database/schema 并自动完成迁移
- 核心业务表与身份相关基础表、对应 repository abstraction 可用
- API 层与持久化层职责分离

## Phase 2: Identity And Admin Roles

目标：补齐最小用户系统、首个管理员引导和平台管理员能力，明确“谁可以创建和管理资源”。

范围：

- 数据库可用但不存在管理员时，进入 Stage 2 setup，而不是直接开放正常控制台
- 首个管理员创建流程
- 首个管理员带 `is_initial_admin` 特殊属性，不能被删除
- 首期只支持 `email` 作为唯一登录标识，不单独引入 `username`
- 密码或 token 认证
- session / access token 基础设施
- 普通用户与管理员权限模型
- 管理员查看全部用户、项目、部署

完成标准：

- 数据库已初始化但还没有管理员时，系统仍停留在 setup 流程直到首个管理员创建完成
- 普通用户只能访问自身资源
- 管理员是用户超集
- 首个管理员不可删除
- 权限检查不散落在 handler 中

## Phase 3: Project And Deployment Domain Model

目标：在身份模型确定后，建立最核心的控制面业务模型，让“某个用户拥有项目，一个项目可以有多次部署”成为系统主线。

范围：

- 项目归属先落到用户
- `projects.owner_user_id` 指向 `users.id`
- `projects`、`deployments`、`deployment_artifacts` 对外 API
- 项目创建、查询、归档 API
- 部署记录查询 API
- 项目状态与部署状态枚举在业务接口中的落地

完成标准：

- 登录用户可以创建并查询自己的项目
- 可以持久化并查询项目对应的部署记录
- API 层继续通过 service/repository 边界访问持久化层

## Phase 4: Build Execution Pipeline

目标：先把“可重复构建”跑通。

范围：

- 构建任务模型
- 构建日志与阶段状态
- 容器执行器抽象
- 首选支持 Podman/Docker，接口设计兼容后续 containerd
- bun 构建约定：安装依赖、执行构建命令、产物收集
- 工作目录隔离、超时、退出码处理

完成标准：

- 能基于本地或测试仓库执行一次 bun 静态站点构建
- 能保存构建日志、状态、产物目录
- 执行器失败路径可测试

## Phase 5: Static Publication And Routing

目标：把产物发布成真正可访问的静态站点。

范围：

- 发布目录布局规范
- 当前生效版本与历史版本切换
- 自定义域名基础模型
- 站点路由抽象
- Caddy adapter：配置文件生成、校验、重载
- 流量统计接口预留与基础计数桩

完成标准：

- 一次成功构建可被发布
- 平台能够切换某项目当前线上版本
- 域名配置链路具备最小闭环

## Phase 6: Teams And Shared Ownership

目标：让团队成为一等公民，支持多成员协作与资源归属。

范围：

- 团队模型
- 团队成员关系
- 团队管理员角色
- 项目归属可为用户或团队
- 团队维度的项目、成员、订阅管理 API

完成标准：

- 团队管理员可以管理团队用户与项目
- 资源归属与权限检查可正确区分“个人项目”和“团队项目”

## Phase 7: Groups, Subscriptions, And Quotas

目标：把配额与商业规则从业务资源中解耦。

范围：

- 用户分组与团队分组模型分开设计
- 分组可绑定额度、节点白名单、可创建团队数、项目数、带宽额度等
- 订阅模型定义“把谁分配到哪些分组”
- 管理员手动分配与订阅自动分配并存
- 配额校验统一由策略服务执行

完成标准：

- 创建项目、创建团队、发布部署等关键动作都经过配额校验
- 分组和订阅变更能立即影响权限和额度

## Phase 8: Source Providers And Webhooks

目标：让平台真正支持“推代码即部署”。

范围：

- Git 仓库连接模型
- 仓库凭据存储
- GitHub webhook
- Gitea webhook
- Forgejo webhook
- 触发策略：指定分支、手动触发、自动触发

完成标准：

- 一次 push 事件可以触发新的部署流程
- provider 差异收敛在 adapter 层

## Phase 9: Multi-Node Control And Execution

目标：把单机方案扩展成主从节点方案。

范围：

- 控制面与节点通信协议
- 节点注册、鉴权、证书或密钥体系
- 心跳与节点状态管理
- 节点角色与标签过滤（节点/主机可以选择 可构建/可部署/可构建可部署/主机模式（只能管理节点）。如果启用某些功能，主机也可以作为节点接到父主机，父主机管理主机，然后主机管理节点（同样的节点也可以是主机））
- 构建任务、部署任务调度
- 失败重试与节点不可用隔离

完成标准：

- 控制面可以把任务分派到具备相应角色的节点
- 通信链路默认加密
- 单机模式与多节点模式共享一套任务抽象

## Phase 10: Operations, Metering, And Hardening

目标：补足可运维性、可观测性和安全边界。

范围：

- 结构化日志
- 指标与健康状态
- 审计日志
- 更完整的限流与防滥用
- 备份/恢复策略
- 数据清理策略
- 配置热更新边界

完成标准：

- 可以定位一次构建/部署失败原因
- 可以追踪管理员关键操作
- 具备上线前基础安全检查清单

## 5. Cross-Cutting Design Tracks

这些不是单独阶段，而是贯穿全程的约束。

### 5.1 Data Model Discipline

- 所有跨子系统关系先画清楚归属关系，再落库。
- 避免在早期表结构里把“用户/团队/分组/订阅/项目”强耦合到一个超级表。
- 优先显式 join 表和枚举状态机。

### 5.2 Permission Model

- 先定义 actor、resource、action，再写接口。
- 管理员权限永远是用户权限超集。
- 团队管理员权限只作用于所属团队。

### 5.3 Quota Evaluation

- 配额计算必须集中在独立 service。
- “创建项目前校验”“部署前校验”“创建团队前校验”都调用同一套策略入口。
- 配额结果需要可解释，便于前端提示与审计。

### 5.4 Execution Isolation

- 构建与部署执行器必须隔离工作目录、日志、超时、资源限制。
- 不直接把 provider webhook 请求映射到容器启动。
- 先入队，再调度，再执行。

### 5.5 Security

- 节点通信默认加密。
- webhook 必须验签。
- 仓库凭据、节点密钥、部署敏感配置都通过统一 secret adapter 管理。

### 5.6 Frontend/API Contract

- 控制台优先消费稳定 REST/JSON API。
- 每个页面先以占位状态接入，确认 API 后再补交互细节。
- 不让前端实现绑死数据库结构。
- setup 阶段也遵守前后端分离；后端不直接返回 setup HTML。
- 控制台统一消费 `/api/v1/*`；setup 相关接口使用 `/api/v1/info` 与 `/api/v1/setup/*`。

## 6. Ordered Slice Backlog

以下是建议按顺序推进的“单轮最小功能”列表。每轮只取一个。

### Foundation

1. 仓库骨架与三应用可运行。
2. 共享 boot config 加载。
3. 缺省 `config.toml` 时的 setup mode 入口与临时监听地址。
4. API 代理前端 dev server。
5. `./public` 优先与 `rust-embed` 回退。
6. `build.rs` 编译期校验嵌入资源。
7. `just` 统一命令与 README。

### Database And Bootstrap Foundation

8. PostgreSQL 结构化配置字段。
9. SeaORM 基础 crate 与数据库连接。
10. 启动时自动准备 schema 与执行迁移。
11. 初始迁移：`projects`、`deployments`、`deployment_artifacts`、`users`、`user_password_credentials`、`user_sessions`。
12. `projects.owner_user_id` 与基础状态枚举。
13. 基础 repository abstraction。
14. setup mode 入口与状态接口。
15. Stage 1 数据库 setup API。
16. Stage 1 完成后的配置落盘、连接验证与迁移推进。
17. API 统一版本化到 `/api/v1/*`。
18. `app/api` 按 VSA/feature 目录重组，并补上 CORS、`tracing`、request logger。

### Identity And Admin Bootstrap

19. `users` 的管理员字段与“首个管理员不可删除”特殊属性。
20. Stage 2 首个管理员创建流程。
21. 密码哈希与凭据写入。
22. session 签发与校验基础设施。
23. 登录接口。
24. 当前用户接口。
25. 管理员查看用户列表。
26. 普通用户 / 管理员鉴权入口。

### Projects And Deployments

27. 项目 service 层与 API 输入输出模型。
28. 创建项目 API。
29. 查询项目列表 API。
30. 记录部署 API。
31. 查询部署列表 API。

### Build Execution

32. 构建任务数据模型。
33. 构建任务状态机。
34. 本地 fake 执行器。
35. Podman/Docker 执行器适配层。
36. bun 静态站点构建流程。
37. 构建日志持久化。
38. 构建超时与取消。

### Static Hosting

39. 部署产物目录规范。
40. 发布当前版本切换。
41. 项目默认子域名生成。
42. 自定义域名模型。
43. Caddy 配置生成器。
44. 代理重载适配器。

### Team

45. 团队模型与迁移。
46. 团队成员关系。
47. 团队管理员角色。
48. 项目归属切换为个人/团队。
49. 团队项目列表接口。

### Groups And Billing Rules

50. 用户分组模型。
51. 团队分组模型。
52. 分组额度字段。
53. 订阅模型。
54. 用户订阅分配分组。
55. 团队订阅分配分组。
56. 创建团队额度校验。
57. 创建项目额度校验。
58. 部署流量额度校验入口。

### Providers

59. Git 仓库连接模型。
60. 手动触发部署。
61. GitHub webhook 验签。
62. GitHub push 触发部署。
63. Gitea webhook 适配。
64. Forgejo webhook 适配。

### Multi-Node

65. 节点模型与迁移。
66. 节点注册接口。
67. 节点鉴权。
68. 节点心跳。
69. 节点角色与标签过滤。
70. 构建任务调度到节点。
71. 部署任务调度到节点。

### Operations

72. 审计日志模型。
73. 结构化日志。
74. 构建/部署指标导出。
75. 管理员操作审计查询。
76. 数据清理任务。

## 7. Milestone Exit Criteria

### M1: Local Demo

- 单机模式可运行
- 首次启动 setup flow 可完成数据库配置与首个管理员创建
- 前端链路稳定

### M2: Single-Node Deployment MVP

- 登录后可管理自己的项目与部署
- 可从仓库手动触发 bun 构建
- 可发布静态产物
- 可绑定基础域名

### M3: Multi-Tenant MVP

- 用户、管理员、团队、项目归属完整
- 分组/订阅/配额对核心动作生效

### M4: Multi-Node MVP

- 控制面可调度不同角色节点
- webhook 可触发自动构建部署

## 8. Current Recommendation

当前 `dev` 已经具备 setup/ready 切换、数据库初始化、首个管理员创建、登录会话、项目控制台、deployment 状态流转，以及 `deployment_artifacts` 的登记 / 查询 API 与前端落点。严格按当前 master plan 检查，`Phase 0` 到 `Phase 3` 已经可以视为收尾完成。

建议下一轮只做一个最小功能：

1. 保持当前 `/api/v1/*` 的 project / deployment / artifact 合同不变，先进入静态发布最小闭环。
2. 先补“当前激活 deployment”模型，让 artifact 已登记但尚未对外生效的状态有明确落点。
3. 在 activation 模型稳定后，再补发布切换与 rollback API。
4. 最后再把静态目录对外访问链路接上，不和 node 执行或 source 自动构建混做。

这样可以继续保持“单轮一个最小功能”的节奏，把当前已经成型的控制面继续推到首个可上线静态版本，而不提前把 node 执行或 source 自动构建缠进来。
