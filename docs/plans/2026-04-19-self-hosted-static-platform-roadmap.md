# Self-Hosted Static Platform Roadmap

## 1. Purpose

这份文档只回答一件事：`grass-worker` 应该按什么顺序做。

它不是 master plan 的替代品，也不是某个单一功能的实现 plan。

职责边界：

- `master plan` 负责定义产品目标、架构原则、系统边界。
- `roadmap` 负责定义阶段顺序、里程碑和完成检查项。
- 单功能 `implementation plan` 负责把某一个最小功能拆成可执行步骤。

当前路线选择采用：

- 单管理员优先
- 纵向切片优先
- 先打通一条完整可用发布链路，再扩多用户、团队、配额、订阅

## 2. Sequencing Principles

- 先做能解锁后续阶段的基础能力，不先堆“看起来重要但暂时不解锁任何东西”的横向设施。
- 每一轮会话只推进一个最小功能，不跨用户、项目、部署、订阅、配额多个子系统混做。
- 先控制面主线，再把 `node` 从占位服务接入真实执行链路。
- 先支持“单管理员可自托管跑通”，再做多用户和产品化能力。
- 先把手动/静态产物发布跑通，再做 Git 源自动构建，避免一开始把 source、scheduler、worker、artifact、routing 一次性缠死。

## 3. Current Status Snapshot

基于当前仓库状态，以下内容已经具备明显基础：

- [x] `app/api` setup / ready 模式切换
- [x] PostgreSQL 配置写回与 migration 基础
- [x] 初始管理员 setup
- [x] `users` / `user_sessions` / `projects` / `deployments` / `deployment_artifacts` 数据模型
- [x] `app/frontend` 静态资源嵌入与开发代理基础
- [x] 登录、会话鉴权、当前用户 API
- [x] 项目管理 API 与控制台
- [x] deployment record 创建 / 查询 / 详情基础
- [ ] deployment 状态流转约束
- [ ] artifact 登记 / 发布激活 / 回滚
- [ ] `node` 真实任务执行
- [ ] Git source 自动构建链路

## 4. Execution Order

下面是明确制作顺序。只有当前阶段达到完成标准，才进入下一阶段。

- [x] Phase 0: Foundation And Bootstrap
- [x] Phase 1: Identity Access Loop
- [x] Phase 2: Project Management Loop
- [ ] Phase 3: Deployment Record Loop
- [ ] Phase 4: First Usable Static Release Loop
- [ ] Phase 5: Delivery Routing And Domain Model
- [ ] Phase 6: Node Agent Integration
- [ ] Phase 7: Source-To-Build Automation
- [ ] Phase 8: Single-Admin Hardening
- [ ] Phase 9: Multi-User Migration
- [ ] Phase 10: Team / Quota / Subscription
- [ ] Phase 11: Production Operations

## 5. Phase Roadmap

### Phase 0: Foundation And Bootstrap

目标：建立 setup、数据库初始化、前端资产链路和控制面骨架。

状态：

- [x] 缺少数据库配置时进入 setup mode
- [x] Stage 1 database setup
- [x] Stage 2 initial admin setup
- [x] 前端 dev proxy / embedded assets
- [x] 核心表和 repository 基础

进入下一阶段条件：

- [x] API 可以稳定进入 `ready` 模式
- [x] 首个管理员可以通过 setup 创建

### Phase 1: Identity Access Loop

目标：让单管理员可以正式登录，系统能稳定识别“当前是谁”。

范围：

- [x] `POST /api/v1/auth/login`
- [x] session 签发与持久化
- [x] `GET /api/v1/me`
- [x] `POST /api/v1/auth/logout`
- [x] 前端登录页
- [x] 前端基于会话的路由守卫

完成标准：

- [x] 初始管理员可以从前端登录
- [x] API 可以返回当前登录用户
- [x] 未登录用户不能访问后续业务页面

为什么先做：

- 项目、部署、发布、日志、回滚都需要清楚资源归属
- 当前仓库已经有 `users` 和 `user_sessions`，这一阶段是最自然的下一步

### Phase 2: Project Management Loop

目标：让登录后的管理员可以管理项目。

范围：

- [x] `POST /api/v1/projects`
- [x] `GET /api/v1/projects`
- [x] `GET /api/v1/projects/:id`
- [x] 项目归档接口
- [x] 前端项目列表页
- [x] 前端项目创建页
- [x] 前端项目详情页

完成标准：

- [x] 管理员可以创建项目
- [x] 管理员可以查看自己的项目列表
- [x] 管理员可以归档项目

为什么在 Phase 1 之后：

- 项目天然依赖用户身份
- 如果跳过认证先做项目，会把 owner 和权限边界做脏

### Phase 3: Deployment Record Loop

目标：先把 deployment 作为控制面对象做完整，不碰自动构建。

范围：

- [x] `POST /api/v1/projects/:id/deployments`
- [x] `GET /api/v1/projects/:id/deployments`
- [x] `GET /api/v1/projects/:id/deployments/:deploymentId`
- [ ] deployment 状态流转约束
- [x] 前端部署列表页
- [x] 前端部署详情页

完成标准：

- [x] 可以在项目下创建 deployment 记录
- [x] 可以查看 deployment 列表和详情
- [ ] deployment 状态流转在 API 层自洽

为什么现在做：

- 先把“发布动作的业务对象”建立起来
- 后续无论手动上传产物还是 node 自动构建，都能挂在这个模型上

### Phase 4: First Usable Static Release Loop

目标：不依赖 `node`，先让一个静态站点真正上线。

范围：

- [ ] deployment artifact 上传或登记接口
- [ ] artifact 元数据与校验信息落库
- [ ] 当前激活 deployment 模型
- [ ] 发布激活接口
- [ ] 回滚接口
- [ ] 静态目录对外访问链路

完成标准：

- [ ] 管理员可以为项目上传静态产物
- [ ] 可以把某个 deployment 激活为线上版本
- [ ] 可以回滚到上一个可用版本
- [ ] 浏览器可以真实访问站点

为什么先于 node：

- 这样先验证控制面、artifact、发布目录、路由模型是否成立
- 否则一上来把 worker 和构建也混进来，问题定位会非常乱

### Phase 5: Delivery Routing And Domain Model

目标：把“站点如何被访问”这件事正式化。

范围：

- [ ] project 到站点访问路径的映射规则
- [ ] SPA fallback 规则和静态资源规则整理
- [ ] 基础域名/主机名模型
- [ ] 404 / index / cache 策略

完成标准：

- [ ] 一个项目的站点访问规则是明确且稳定的
- [ ] 静态站点和 SPA 场景都能解释清楚

### Phase 6: Node Agent Integration

目标：把 `app/node` 从占位服务接入真实执行链路。

范围：

- [ ] control-plane 与 node 的认证方案
- [ ] deployment task claim / poll 协议
- [ ] node 心跳和能力上报
- [ ] node 更新 deployment 状态
- [ ] node 上传或登记 artifact
- [ ] 基础执行日志回传

完成标准：

- [ ] node 可以领取任务
- [ ] node 可以驱动 deployment 从 `pending` 走到 `ready` 或 `failed`
- [ ] 控制台能看到 node 执行结果

### Phase 7: Source-To-Build Automation

目标：从“手动上传产物”升级到“从 source 自动构建产物”。

范围：

- [ ] 首个 source 类型定稿
- [ ] source revision / snapshot 模型
- [ ] build command / install command / output dir 模型
- [ ] node checkout / build / collect artifact 合同
- [ ] 构建失败日志展示
- [ ] 构建完成后自动衔接发布

完成标准：

- [ ] 管理员可以从 source 发起真实构建
- [ ] 构建成功后可以形成可发布 artifact
- [ ] 构建失败时日志和状态可见

### Phase 8: Single-Admin Hardening

目标：把单管理员版本从“能跑”提升到“能持续使用”。

范围：

- [ ] deployment cancel / retry
- [ ] artifact retention / cleanup
- [ ] 并发部署规则
- [ ] 操作审计日志
- [ ] 常见失败恢复路径
- [ ] 安装与升级文档

完成标准：

- [ ] 单管理员可以长期维护多个项目
- [ ] 常见失败场景都有恢复路径

### Phase 9: Multi-User Migration

目标：从单管理员产品迁移到真正的多用户控制面。

范围：

- [ ] 普通用户创建/登录策略
- [ ] project ownership 权限检查补全
- [ ] session + resource authorization 收口
- [ ] 角色模型
- [ ] UI 按角色裁剪操作

完成标准：

- [ ] 多用户可以共存
- [ ] 用户之间无法越权访问项目和部署

### Phase 10: Team / Quota / Subscription

目标：补齐产品化层能力。

范围：

- [ ] team / workspace 模型
- [ ] 项目共享与成员管理
- [ ] 使用量统计
- [ ] quota enforcement
- [ ] subscription / plan 模型
- [ ] 管理端能力

完成标准：

- [ ] 平台具备团队化能力
- [ ] 平台具备基础商业化能力

### Phase 11: Production Operations

目标：把 self-hosted 平台提升到可正式运维。

范围：

- [ ] metrics / tracing / dashboard
- [ ] 备份与恢复
- [ ] 安全加固
- [ ] 升级兼容策略
- [ ] release checklist

完成标准：

- [ ] 平台可作为正式 self-hosted 产品交付
- [ ] 运维侧有明确观察、恢复和升级路径

## 6. Milestone View

### Milestone A: Can Install

- [x] 可以完成 setup
- [x] 可以创建初始管理员
- [x] 可以正式登录

### Milestone B: Can Manage Projects

- [x] 可以创建项目
- [x] 可以查看项目
- [x] 可以归档项目

### Milestone C: Can Publish Static Sites Manually

- [x] 可以创建 deployment
- [ ] 可以上传或登记 artifact
- [ ] 可以激活和回滚发布
- [ ] 可以真实访问静态站点

### Milestone D: Can Build And Publish Automatically

- [ ] node 可以领取任务
- [ ] node 可以执行构建
- [ ] 构建结果可以自动发布

### Milestone E: Can Serve Multiple Users

- [ ] 普通用户体系可用
- [ ] 权限边界成立
- [ ] 团队共享可用

### Milestone F: Can Operate As A Product

- [ ] quota / subscription 可用
- [ ] metrics / tracing 可用
- [ ] 备份、恢复、升级策略明确

## 7. Next Planning Rule

roadmap 确认后，后续工作一律按这个规则继续：

- [ ] 只从当前未完成的最早 phase 中挑一个最小功能
- [ ] 先写该最小功能的 implementation plan
- [ ] 实现、测试、回顾完成后再进入同 phase 的下一个最小功能
- [ ] 不跨 phase 并行推进

当前应该进入的阶段：

- [ ] Phase 3: Deployment Record Loop

当前建议优先拆出的第一个最小功能：

- [ ] deployment 状态流转约束
