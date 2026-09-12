# grass-worker TODO

当前版本：0.1.0

## 使用规则

- 实现工作必须映射到本文档中的优先级和小功能。
- 完成并合并的项目应从本文档删除，不保留完成历史。
- 未经用户批准，不得把较低优先级或 Future 项目提前并入当前功能。
- 当前版本从本文档头部读取，并用于 GitHub Milestone 与 Project 命名。

# Control API 规范整理

## R1：路径切片、审计与类型规范

父事项：[#207](https://github.com/Grass-Development-Team/grass-worker/issues/207)。按已批准计划依次实施；完成并合并的实现小项按本文规则移除，最后执行 R1.9 删除整个区块。

- R1.1 定义 URL 路径与模块归属、本地 Request/Response、路由组合和兼容性基线。[#208](https://github.com/Grass-Development-Team/grass-worker/issues/208)
- R1.2 将结构化日志、审计内容、脱敏、请求上下文、写入、查询和保留策略统一到 `infra/audit`；明确事务与失败语义。[#209](https://github.com/Grass-Development-Team/grass-worker/issues/209)
- R1.3 按 URL 路径整理项目部署、internal Build/Serve 与预览授权接口，使用本地 DTO 并验证 Node 协议。[#210](https://github.com/Grass-Development-Team/grass-worker/issues/210)
- R1.4 按相同规则整理其余 auth、me、teams、admin、setup、avatars 等接口。[#211](https://github.com/Grass-Development-Team/grass-worker/issues/211)
- R1.5 明确共享业务与技术适配器归属，消除其他接口和后台任务对 controller 模块的工具依赖。[#212](https://github.com/Grass-Development-Team/grass-worker/issues/212)
- R1.6 统一 PATCH 缺失/null/值、状态转换和错误映射，修复个人显示名清空并保留安全诊断。[#213](https://github.com/Grass-Development-Team/grass-worker/issues/213)
- R1.7 整理测试位置与共享 fixture，改善 SQL、JSON、宏和声明的人工可读性。[#214](https://github.com/Grass-Development-Team/grass-worker/issues/214)
- R1.8 统一 workspace 依赖继承，并验证证书解析、签发和交付相关行为。[#215](https://github.com/Grass-Development-Team/grass-worker/issues/215)
- R1.9 在上述实现全部完成、验证并合并后，删除整个 R1 TODO 区块（包括本项），保留其他未完成范围。[#216](https://github.com/Grass-Development-Team/grass-worker/issues/216)

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
