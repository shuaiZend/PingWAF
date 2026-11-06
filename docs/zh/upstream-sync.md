# 上游同步分析（2026-11-07）

本仓库内置的 `pingap-*` 与上游 `vicanso/pingap` 差多远、上游自上次同步以来
修了什么、下一步该搬什么。一句话结论：**两边没有共同 git 祖先**，无法
`merge` 上游，每次同步都是手工移植。本文记录基线与待办，避免下次从猜测开始。

## 版本

| 组件 | PingWAF | 上游 | 说明 |
|---|---|---|---|
| Pingora | `0.9.1`（本分支） | `0.9.1`（2026-11-03） | 补丁升级，见 CHANGELOG |
| pingap crates | `0.32.17`（工作区版本） | `0.16.1` / `main@cfdf5f76` | 上游有自己的版本线 |

`git merge-base HEAD upstream/main` 返回空：两条历史无关（本工作区是重新导入，
不是 fork）。`git merge upstream/main` 会重放约 2000 个无关提交，**永远不是
正确做法**。

## 基线

本地不是任何一个上游 tag 的快照。按 `pingap-*` 比较，内容最接近的是
**`v0.15.0`（`8270a1eb`）**：

| 上游 ref | 变更文件 | 新增 | 删除 | 仅修改文件行数 |
|---|---|---|---|---|
| **`v0.15.0`** | 120 | +35,637 | −4,973 | **14,735（最小）** |
| `v0.14.3` | 99 | +37,970 | −1,631 | 15,020 |
| `v0.16.0` | 200 | +36,327 | −57,744 | 63,354 |
| `v0.16.1` | 208 | +36,431 | −62,246 | 65,211 |
| `main` | 223 | +36,642 | −67,136 | 69,666 |

最后一列只统计**两边都存在**的文件的新增+删除行（`--diff-filter=M`），排除
新增/删除文件，避免一侧的模块重写主导分数。`v0.16.x` 重写了插件基座、错误
处理与日志（因此有 −57k 删除），所以 `v0.15.0` 是最后一个还能逐文件对读的
版本，`v0.14.3` 紧随其后。本地还包含更晚的上游代码（`error_page.rs`），同时
缺少部分 `v0.15.0` 文件（`ja4.rs`、`first_round.rs`），因此基线只是"最接近"，
不是"祖先"。

重新生成：

```bash
git fetch upstream '+refs/tags/v0.1*:refs/upstream-tags/v0.1*'
git diff --shortstat refs/upstream-tags/v0.15.0 HEAD -- 'pingap-*'
git diff --diff-filter=M --numstat refs/upstream-tags/v0.15.0 HEAD -- 'pingap-*' \
  | awk '{a+=$1; d+=$2} END {print a+d " modified-file lines"}'
git log --oneline refs/upstream-tags/v0.15.0..upstream/main -- 'pingap-*'
```

（`refs/upstream-tags/*` 用于避开本地同名的 `v0.x.y` 发布 tag。）

## 移植清单

`v0.15.0..upstream/main` 共有 69 个触及 `pingap-*` 的提交（24 fix / 18 feat /
15 test / 4 refactor / 3 perf / 2 docs / 2 chore / 1 ci）。下表只列会改变实际
行为的项。"本地"表示这项防护是否已经存在。

### P0 — 安全 / 请求路由

| 上游提交 | 修复内容 | 本地 | 动作 |
|---|---|---|---|
| `fd6fe1a3` | location 按**归一化路径**匹配（`/%61dmin`、`//admin`、`/public/../admin` 不再绕过 `/admin` 的插件） | **缺失**（无 `normalize_path`） | 优先移植，并带上上游单测矩阵 |
| `cc2e68a0` | 16 项审计问题：admin 鉴权绕过、缓存键缺 host、目录遍历、ACME token 泄露、可信代理后 IP 可伪造、健康后端仍 503 等 | 多数已具备（缓存键含 host：`pingap-proxy/src/server.rs:2261`），其余逐项复核 | 按项复核，补齐缺口 |
| `44e75ed5`、`22d4196a` | 非 ASCII XFF 伪造客户端 IP；h2 rewrite 后缓存键丢 host；ipv4-mapped 前缀按 IPv4 匹配 | 实现不同（自研 `ProxyTrust`） | 对照审计 `ProxyTrust` |
| `2d895b21` | 合并拆分的 `Cookie` 头；拒绝既非 path 也非 URL 的目标 | 缺失 | 移植 |

### P1 — 稳定性 / 正确性

| 上游提交 | 修复内容 | 本地 | 动作 |
|---|---|---|---|
| `0f7a981b` | Pingora 0.9 的 brotli 流不收尾（严格解码器拒收 `br` 响应） | 缺失（直接用 pingora `ResponseCompression`） | 随 0.9.1 一并移植 `BrotliTail` |
| `b29acc17` | ACME：每次 CA/DNS 交互加超时（一个挂住的请求会停掉整个续期服务）；接受 `ready` order；按可注册域取 zone；自签证书上限 | 超时缺失 | 至少移植超时包装 |
| `e034c140` | 同一 location 两个 inflight limit 都要计数；每实例独立 body handler；JWT `nbf`；redirect 端口 | 缺失（`ctx.state.guard` 只有一个） | 改 `limit.rs`/`jwt.rs` 时顺带移植 |

### P2 — 加固与功能

`e3aeab27`、`56ee9ed9`、`8aa7f853`、`4b402362`、`dc2a540e` 增加配置校验与
reload 加固；`8082ab84` 拒绝重复插件 category。功能类（`maintenance`、
`uri_block`、`mirror`、`request_headers`、`bandwidth_limit`、JA4、
`hmac_auth`、unix socket、到上游的 PROXY protocol、`strict_sni` 421）**未
移植**，见 `UPSTREAM.md`；是否移植是产品决策，不是同步义务。

## 本次发现的上游缺陷

本项目已经规避、但 `upstream/main` 仍存在的三个缺陷，已写成英文报告放在
[`docs/upstream/`](../upstream/README.md)（面向上游提交）：

1. 透明池 `sni = "$host"` 直接把原始 Host 当 SNI（含 IP 字面量与大小写）；
2. 每个未匹配 SNI 的握手打一条 `ERROR` 日志（未认证即可制造日志洪水）；
3. `X-Pingap-EType` / `{{error_type}}` 回显内部 `Custom` 错误名。

## 推荐顺序

1. `fd6fe1a3` 路径归一化（P0，范围小、易测试）；
2. `0f7a981b` 的 `BrotliTail`（用户可见的 `br` 正确性）；
3. `b29acc17` ACME 超时（可用性）；
4. `2d895b21` Cookie 合并 + 可路由目标校验；
5. 每次上游发版后重跑本文的分析，持续更新清单，而不是散落在聊天记录里。

每个移植都应带上上游已有测试（若有）与本地回归测试；不得无故改变
`pingap-*` 的公共签名，否则需同步整个工作区。
