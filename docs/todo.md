# grass-worker TODO

当前版本：0.1.0

> 状态（2026-07-27）：第一阶段已交付。本文档是未完成工作、当前优先级和后续范围的唯一来源；已完成能力不在这里重复维护。

## 使用规则

- 实现工作必须映射到本文档中的优先级和小功能。
- 完成并合并的项目应从本文档删除，不保留完成历史。
- 未经用户批准，不得把较低优先级或 Future 项目提前并入当前功能。
- 当前版本从本文档头部读取，并用于 GitHub Milestone 与 Project 命名。

# 第二阶段

## P0：Git 源码访问、存量数据与回归保护

父跟踪 Issue：`#89`

### S2-P0.1 Git transport 与私有源码凭据

跟踪 Issue：`#90`

- 通过统一 Git Source Adapter 支持 HTTP、HTTPS、SSH、scp-like SSH 和 `git://` 仓库地址；
- 所有 transport 支持显式任意端口；
- HTTP 和 `git://` 仅允许匿名公开仓库；
- HTTPS 支持 username + PAT、部署令牌或服务端仍支持的密码；
- SSH 支持私钥和可选 passphrase，不支持 SSH 密码登录；
- 支持团队级凭据和项目专属凭据覆盖；
- 只有团队 owner/admin 可以创建、轮换、撤销和绑定凭据；
- 凭据按 scheme、host、port 约束，不得写入仓库 URL；
- 凭据 payload 使用独立 master key 做版本化认证加密；
- Deployment 创建时固定凭据版本，普通轮换不影响已排队 Deployment，主动撤销立即阻止旧版本使用；
- Node 通过绑定 node、deployment 和 credential version 的一次性短期 lease 获取凭据；
- SSH host key 首次使用时展示指纹并要求 owner/admin 明确批准，变化后必须重新批准；
- 默认只允许全球可路由公网目标，非公网目标需要 Node 管理员按 host/IP + port 配置精确例外；
- 所有 URL、DNS/IP、网络、凭据和 host-key 检查必须由 Control API 与 Node 分层执行；
- 凭据不得出现在 URL、API 响应、Deployment 快照、日志或审计 metadata 中；
- Console 提供团队凭据管理、项目绑定、host-key 审批和稳定错误提示。

### S2-P0.2 repository_url 存量迁移与文档整合

跟踪 Issue：`#91`

- 保留合法 HTTP、HTTPS、SSH、scp-like SSH 和 `git://` 存量地址；
- 清理 `file://`、`ext://`、本地路径和其他危险/不支持地址；
- 在 `source_config` 中保存安全的迁移原因，Console 不回显危险原值；
- 迁移必须幂等，并覆盖实际 PostgreSQL schema 验证；
- 未完成内容只在本文档维护；
- 仓库协议、版本与范围规则只引用本文档。

### S2-P0.3 安全修复回归保护

跟踪 Issue：`#92`

- 覆盖 cancel 路径只释放一次并发构建槽；
- 覆盖没有 per-build timeout 时回退到 Node `command_timeout_seconds`；
- 覆盖 archive 解压条目数、单条目字节数和总字节数上限；
- 增加 Git URL、网络策略、凭据加密、租约、权限、host-key 和迁移回归测试；
- 完成后运行针对性测试和 `just quality`。

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
