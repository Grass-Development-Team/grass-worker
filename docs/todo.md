# grass-worker TODO

当前版本：0.1.0

> 状态（2026-08-01）：第一阶段和第二阶段平台治理、交付防护已交付。本文档是未完成工作、当前优先级和后续范围的唯一来源；已完成能力不在这里重复维护。

## 使用规则

- 实现工作必须映射到本文档中的优先级和小功能。
- 完成并合并的项目应从本文档删除，不保留完成历史。
- 未经用户批准，不得把较低优先级或 Future 项目提前并入当前功能。
- 当前版本从本文档头部读取，并用于 GitHub Milestone 与 Project 命名。

# 第二阶段

## P1：产品能力

- P1.4 Custom Grass Output：允许用户提供 `.grass/output/output.toml`，并定义 manifest 安全校验策略；
- P1.6 消息外发：Webhook；
- P1.7 更多 DNS Provider：增加 DNSPod、Route53 等实现；
- P1.8 独立 Host Source：允许 Preview 与 Production 使用独立的默认 Host Source；
- P1.12 用户与团队头像：支持浏览器裁剪、上传、替换与移除，并以 WebP 对象存储；
- P1.13 部署预览截图：可选 Chromium provider 为 Production Deployment 生成 WebP 预览 artifact；

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
