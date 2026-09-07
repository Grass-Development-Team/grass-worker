# grass-worker TODO

当前版本：0.1.0

## 使用规则

- 实现工作必须映射到本文档中的优先级和小功能。
- 完成并合并的项目应从本文档删除，不保留完成历史。
- 未经用户批准，不得把较低优先级或 Future 项目提前并入当前功能。
- 当前版本从本文档头部读取，并用于 GitHub Milestone 与 Project 命名。

# 第二阶段

## P1：产品能力

- P1.1 区域标签与区域部署调度：为 Node 增加 `region` 配置；部署请求可以指定目标区域；调度器只从该区域内健康且具备 Serve 能力的节点中按 CPU、内存、磁盘和部署槽位择优分配；节点保留全量路由快照，并可通过单跳 Peer Hop 代理访问实际部署在其他节点或区域的站点。
- P1.2 区域 CNAME 接入：为每个区域提供独立的接入域名；自定义域名绑定时生成对应区域的 CNAME 指引；入口节点通过客户端原始 Host 匹配 Deployment；记录多节点入口、健康检查和回源 Host 保留要求，确保区域入口故障时可以切换到同区域存活节点。CNAME 提供自动 ssl 证书管理和更新（基于 zerossl / Let's Encrypt，可在后台调控），基于 txt 的域名认证等等。
- P1.3 网关通信鉴权模式：将 `gateway_token` 设计为可选配置；内网或受防火墙保护的部署可以免密通信；公网异构节点可以启用网关 token，或通过 Tailscale、WireGuard 等加密网络保护节点间流量；无 token 模式仍必须保留 Host 路由和单跳次数限制。
- P1.4 更多 DNS Provider：增加 DNSPod、Route53 等实现。

## P2：平台扩展

- P2.1 Serve 自动故障转移：Control API 检测 Serve 节点心跳超时后，在同区域健康节点中重新分配部署；目标节点从对象存储或可用 artifact 源拉取产物，完成启动、健康检查和路由快照切换；入口节点和跨区 Peer Hop 在切换期间保持可用，并支持失败重试和人工恢复。
- P2.2 Control API 高可用：支持多 Control API 协调、选主、状态一致性和故障切换，避免单一 Control API 成为调度与路由控制的单点故障。
- P2.3 分布式构建缓存：在多个 Build Node 之间共享可校验、可失效的构建缓存，并保持租户隔离与缓存命中结果可追踪。
- P2.4 Serverless Functions、Edge Runtime、ISR、Middleware 和 hybrid runtime。
- P2.5 Apple Container 与 Jail backend。
- P2.6 Vercel Output API 深度兼容。
- P2.7 Custom Grass Output：允许用户提供 `.grass/output/output.toml`，并定义 manifest 安全校验策略。

# Future

- 在线支付、发票、订阅扣款和商业化结算；
- GitHub App 深度集成；
- 中国境内备案状态、访问控制、停止页面、套餐与配额自动放行。
