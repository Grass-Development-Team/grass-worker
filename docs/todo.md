# grass-worker TODO

当前版本：0.1.0

> 状态（2026-07-27）：第一阶段已交付。本文档是未完成工作、当前优先级和后续范围的唯一来源；已完成能力不在这里重复维护。

## 使用规则

- 实现工作必须映射到本文档中的优先级和小功能。
- 完成并合并的项目应从本文档删除，不保留完成历史。
- 未经用户批准，不得把较低优先级或 Future 项目提前并入当前功能。
- 当前版本从本文档头部读取，并用于 GitHub Milestone 与 Project 命名。

# 第二阶段

## P0：平台管理、审计与权限修复

- P0.1 审计基础：覆盖用户 API、登录失败和权限拒绝，记录操作者、时间、来源、结果、耗时、脱敏变更与可见级别；默认保留 90 天；
- P0.2 平台与团队差异化审计：服务端分页、筛选、详情和权限隔离；团队仅 Owner/Admin 可查看当前团队精选业务事件；
- P0.3 Console 角色能力矩阵：Viewer 隐藏无权限操作，配置页面保持只读可见，后端继续独立鉴权；
- P0.4 动态站点品牌：登录、注册、邀请、侧栏和标题使用配置名称，仅弱化保留 Grass Worker 与版本署名；
- P0.5 Invitation 预检：进入链接即显示团队、角色和有效期，并立即提示失效、已使用或邮箱不匹配；
- P0.6 Team Group Review Policy：仅平台管理员配置，按 Team Group Policy > 平台默认解析，团队不可覆盖；
- P0.7 Control API 非敏感配置管理：Console 可读写并校验，敏感配置仅显示配置状态；
- P0.8 Node 期望配置同步：非敏感 Node 配置可由 Console 管理，并显示 Pending、Applying、Applied、Failed；
- P0.9 Node 排空、迁移与删除队列：Serve 安全迁移并切换路由，Build 等待任务完成，统一显示删除进度与失败恢复；

## P1：测试与工程质量

- 补齐 Console 测试：setup、login、team switcher、deployment list/detail、quota、protected route；
- 优先覆盖 log viewer 的 seq 去重和连续水位补拉；
- 按路由动态 import 拆分 admin、deployments、setup 等 feature chunk，消除主 bundle 超过 500 KB 的警告；
- 增加最小 Vite 仓库 checkout -> build -> output -> serve 的 Node CI 冒烟；该检查依赖 CI 中可用的容器运行时。

## P2：产品能力

- Artifact 清理策略：日志保留天数、Preview/失败部署保留期、Production 最近 N 个、定时任务和 active deployment 保护；
- SSR 配额增强：最大 SSR 进程数和每月 SSR 运行小时数；
- 更多 SSR 框架：SvelteKit adapter-node、Remix / React Router SSR；
- Custom Grass Output：允许用户提供 `.grass/output/output.toml`，并定义 manifest 安全校验策略；
- 认证增强：OAuth、MFA、密码策略和密码找回；
- SMTP、邀请邮件、部署结果通知和 Webhook；
- 更多 DNS Provider 实现，例如 DNSPod 和 Route53。

## P3：平台扩展

- S3、MinIO、R2 artifact storage；
- build node 与 serve node 分离调度；
- artifact 跨节点同步；
- 多节点负载均衡和 failover；
- 分布式构建缓存；
- Serverless Functions、Edge Runtime、ISR、Middleware 和 hybrid runtime；
- Apple Container 与 Jail backend；
- Vercel Output API 深度兼容；
- Preview 与 Production 独立默认 Host Source。

# Future

- 在线支付、发票、订阅扣款和商业化结算；
- GitHub App 深度集成；
- 中国境内备案状态、访问控制、停止页面、套餐与配额自动放行。
