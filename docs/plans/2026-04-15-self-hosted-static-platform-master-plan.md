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

## Phase 0: Foundation And Bootstrap

目标：建立 setup、数据库初始化、前端资产链路和控制面骨架，保证仓库在缺省配置与开发模式下都能稳定启动。

范围：

- Rust workspace 与三应用目录结构
- `app/api` / `app/node` / `app/frontend` 基础骨架与 `/health`
- `config.toml` 启动引导与 `[development].dev_server` 代理
- 缺省配置时的 setup mode 入口与 runtime mode 切换
- `./public` 优先 + `crates/assets` 嵌入回退
- SeaORM 迁移基础设施与 Stage 1 database setup
- Stage 2 initial admin setup 前的数据库引导链路
- `users` / `user_sessions` / `projects` / `deployments` / `deployment_artifacts` 基础表与 repository abstraction
- `just` 命令统一开发流程

完成标准：

- 缺少数据库配置时，`app/api` 可以进入 setup 并完成数据库初始化
- `app/api` 可以稳定进入 `ready` 模式
- release 构建产物包含可用前端资源与数据库迁移基础

## Phase 1: Identity Access Loop

目标：让单管理员可以正式登录，系统能稳定识别“当前是谁”。

范围：

- 首个管理员创建完成后的登录闭环
- 基于 `email` 的密码认证
- session 签发、持久化、读取与注销
- `POST /api/v1/auth/login` / `GET /api/v1/me` / `POST /api/v1/auth/logout`
- 前端登录页与基于会话的路由守卫
- handler 之外的身份读取与权限边界收口

完成标准：

- 初始管理员可以从前端正式登录
- API 可以稳定返回当前登录用户
- 未登录用户不能访问后续业务页面

## Phase 2: Project Management Loop

目标：让登录后的管理员可以管理项目，并具备最小平台管理视角。

范围：

- `projects.owner_user_id` 指向 `users.id`
- 项目创建、查询、详情、更新 API
- 项目归档、取消归档、软删除、恢复、转移 owner、硬删除
- 前端项目列表页、项目创建入口、项目详情页
- 管理员恢复/清理已删除项目的控制台入口
- 管理员查看全部用户、项目、部署的最小能力

完成标准：

- 管理员可以创建并查看自己的项目
- 管理员可以完成项目生命周期管理
- 管理员具备最小 user/project/deployment 管理视角

## Phase 3: Deployment Record Loop

目标：先把 deployment 作为控制面对象做完整，不碰自动构建。

范围：

- `POST /api/v1/projects/:id/deployments`
- `GET /api/v1/projects/:id/deployments`
- `GET /api/v1/projects/:id/deployments/:deploymentId`
- deployment 状态流转约束
- 前端部署列表页与部署详情页
- API 继续通过 service / repository 边界访问持久化层

完成标准：

- 可以在项目下创建 deployment 记录
- 可以查看 deployment 列表和详情
- deployment 状态流转在 API 层自洽

## Phase 4: First Usable Static Release Loop

目标：不依赖 `node`，先让一个静态站点真正上线。

范围：

- deployment artifact 上传或登记接口
- artifact 元数据、校验信息与存储路径落库
- 当前激活 deployment 模型
- 发布激活接口
- 回滚接口
- 静态目录对外访问链路

完成标准：

- 管理员可以为项目上传或登记静态产物
- 可以把某个 deployment 激活为线上版本
- 可以回滚到上一个可用版本
- 浏览器可以真实访问静态站点

## Phase 5: Delivery Routing And Domain Model

目标：把“站点如何被访问”这件事正式化。

范围：

- project 到站点访问路径的映射规则
- SPA fallback / 静态资源 / 404 / index / cache 策略整理
- 基础域名/主机名模型
- 站点路由抽象
- 路由配置 adapter（例如 Caddy 配置生成、校验、重载）

完成标准：

- 一个项目的站点访问规则是明确且稳定的
- 静态站点和 SPA 场景都能解释清楚
- 基础域名配置链路具备最小闭环

## Phase 6: Node Agent Integration

目标：把 `app/node` 从占位服务接入真实执行链路。

范围：

- control-plane 与 node 的认证方案
- deployment task claim / poll 协议
- node 心跳和能力上报
- node 更新 deployment 状态
- node 上传或登记 artifact
- 基础执行日志回传

完成标准：

- node 可以领取任务
- node 可以驱动 deployment 从 `pending` 走到 `ready` 或 `failed`
- 控制台能看到 node 执行结果

## Phase 7: Source-To-Build Automation

目标：从“手动上传产物”升级到“从 source 自动构建产物”。

范围：

- 首个 source 类型定稿
- source revision / snapshot 模型
- build command / install command / output dir 模型
- node checkout / build / collect artifact 合同
- 构建失败日志展示
- 构建完成后自动衔接发布

完成标准：

- 管理员可以从 source 发起真实构建
- 构建成功后可以形成可发布 artifact
- 构建失败时日志和状态可见

## Phase 8: Single-Admin Hardening

目标：把单管理员版本从“能跑”提升到“能持续使用”。

范围：

- deployment cancel / retry
- artifact retention / cleanup
- 并发部署规则
- 操作审计日志
- 常见失败恢复路径
- 安装与升级文档

完成标准：

- 单管理员可以长期维护多个项目
- 常见失败场景都有恢复路径

## Phase 9: Multi-User Migration

目标：从单管理员产品迁移到真正的多用户控制面。

范围：

- 普通用户创建/登录策略
- project ownership 权限检查补全
- session + resource authorization 收口
- 角色模型
- UI 按角色裁剪操作

完成标准：

- 多用户可以共存
- 用户之间无法越权访问项目和部署

## Phase 10: Team / Quota / Subscription

目标：补齐产品化层能力。

范围：

- team / workspace 模型
- 项目共享与成员管理
- 使用量统计
- quota enforcement
- subscription / plan 模型
- 管理端能力

完成标准：

- 平台具备团队化能力
- 平台具备基础商业化能力

## Phase 11: Production Operations

目标：把 self-hosted 平台提升到可正式运维。

范围：

- metrics / tracing / dashboard
- 备份与恢复
- 安全加固
- 升级兼容策略
- release checklist

完成标准：

- 平台可作为正式 self-hosted 产品交付
- 运维侧有明确观察、恢复和升级路径

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
