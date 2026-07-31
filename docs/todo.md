# grass-worker TODO

当前版本：0.1.0

> 状态（2026-07-31）：第一阶段和第二阶段平台治理、交付防护已交付。本文档是未完成工作、当前优先级和后续范围的唯一来源；已完成能力不在这里重复维护。

## 使用规则

- 实现工作必须映射到本文档中的优先级和小功能。
- 完成并合并的项目应从本文档删除，不保留完成历史。
- 未经用户批准，不得把较低优先级或 Future 项目提前并入当前功能。
- 当前版本从本文档头部读取，并用于 GitHub Milestone 与 Project 命名。

# 第二阶段

## P0：发布质量与回归防线

- P0.1 Console 关键路径覆盖：补齐 Setup 主流程、Quota 和 Protected Route 测试；
- P0.2 Log Viewer 连续性测试：覆盖 WebSocket 与 HTTP 补拉的 seq 去重、乱序合并、缺口检测和连续水位推进；
- P0.3 Console 路由级拆包：对 Admin、Deployments、Setup 等 feature 使用动态 import，消除主 bundle 超过 500 KB 的现有警告；
- P0.4 Node 构建冒烟：在 CI 中用最小 Vite 仓库完成 checkout -> build -> output -> serve，验证容器运行时和真实静态交付链路；

## P1：产品能力

- P1.1 Artifact 清理策略：日志保留天数、Preview/失败部署保留期、Production 最近 N 个、定时任务和 active deployment 保护；
- P1.2 SSR 配额增强：最大 SSR 进程数和每月 SSR 运行小时数；
- P1.3 更多 SSR 框架：SvelteKit adapter-node、Remix / React Router SSR；
- P1.4 Custom Grass Output：允许用户提供 `.grass/output/output.toml`，并定义 manifest 安全校验策略；
- P1.5 认证增强：OAuth、MFA、密码策略和密码找回；
- P1.6 消息外发：SMTP、邀请邮件、部署结果通知和 Webhook；
- P1.7 更多 DNS Provider：增加 DNSPod、Route53 等实现；
- P1.8 独立 Host Source：允许 Preview 与 Production 使用独立的默认 Host Source；

## P2：平台扩展

- P2.1 Artifact 对象存储：S3、MinIO、R2 backend；
- P2.2 Serve 自动故障转移：多 Serve 节点之间自动重新分配部署并完成路由切换；
- P2.3 Control API 高可用：多 Control API 协调、选主和一致性保障；
- P2.4 分布式构建缓存；
- P2.5 Serverless Functions、Edge Runtime、ISR、Middleware 和 hybrid runtime；
- P2.6 Apple Container 与 Jail backend；
- P2.7 Vercel Output API 深度兼容；

# Future

- 在线支付、发票、订阅扣款和商业化结算；
- GitHub App 深度集成；
- 中国境内备案状态、访问控制、停止页面、套餐与配额自动放行。
