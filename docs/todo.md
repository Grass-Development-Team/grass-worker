# grass-worker TODO

当前版本：0.1.0

## 使用规则

- 实现工作必须映射到本文档中的优先级和小功能。
- 完成并合并的项目应从本文档删除，不保留完成历史。
- 未经用户批准，不得把较低优先级或 Future 项目提前并入当前功能。
- 当前版本从本文档头部读取，并用于 GitHub Milestone 与 Project 命名。

# 当前维护

## P2：代码清理

- P2.8 清理无用 Rust 代码、旧 DNS challenge 写入、过期 lint 豁免及未使用依赖；限定测试替身的编译范围，并简化无效包装。跟踪：#196。
- P2.9 统一存储配置解析与 Console 字段模型，保留首次设置和在线迁移各自的事务流程。跟踪：#197。
- P2.10 集中 HTTP 数据库和缓存依赖获取，保留操作名及错误语义。跟踪：#198。
- P2.11 统一域名删除编排、按删除代次释放配额及路由失效通知。跟踪：#199。
- P2.12 清理 Console 旧 API 和多余导出，复用设置开关、导航、账号菜单及安全时间展示。跟踪：#200。

本轮对应代码清理审查，父任务为 #195；内部模块没有仓外消费者，允许调整内部接口。

# 第二阶段

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
