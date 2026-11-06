# 上游缺陷报告

这里汇总维护本工作区内置 `pingap-*` crates 时确认的
**`vicanso/pingap`** 缺陷。为便于直接提交上游，每份报告都使用英文书写，
并附 `upstream/main` 的代码位置（行号为写作时的快照，可能随上游变动）。

| 报告 | 级别 | 上游状态 |
|---|---|---|
| [透明池把原始 Host 当作 SNI](../../upstream/2026-11-07-transparent-pool-sni.md) | P2（互操作 / 连接池浪费） | 尚未提交 |
| [未匹配 SNI 每次握手打一条 ERROR](../../upstream/2026-11-07-unmatched-sni-log-flood.md) | P2（日志洪水） | 尚未提交 |
| [`X-Pingap-EType` 泄露内部错误名](../../upstream/2026-11-07-x-pingap-etype-leak.md) | P3（信息泄露） | 尚未提交 |

## 使用方式

- 提交前先在最新 `upstream/main` 上复现；行号只是快照。
- 一份报告对应一个 issue；建议标签 `bug`，只有报告明确说明安全影响时才加
  `security`。
- 报告描述的是缺陷而非利用方式，不包含任何站点、地址或客户数据。
- 若上游已修复，请直接删除对应文件，不要提交重复 issue。

## 这些报告怎么来的

本工作区的 `pingap-*` 是长期手工移植而非 git fork（与上游无共同祖先）。
2026-11 的同步审查把本地修复与 `upstream/main` 逐条对比，只保留下仍能在
上游复现的项。对比方法与移植待办见
[`docs/zh/upstream-sync.md`](../upstream-sync.md)。
