# sub_filter

在响应体中搜索替换，类似 nginx 的 `sub_filter` 与 `subs_filter` 模块。适用于改写绝对 URL、注入 script 标签，或修补无法修改的上游。

- **步骤：** `response` 与 `response_body`
- **注册名：** `sub_filter`

## 配置

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `category` | string | — | 必须为 `sub_filter`。 |
| `filters` | string[] | `[]` | 替换规则；见下方语法。 |
| `path` | string | — | 请求路径上的正则。未设置表示所有路径。 |
| `status_codes` | string | — | 逗号分隔的状态码，如 `"200,201"`。未设置表示全部。 |

## 规则语法

```
sub_filter  '<literal>' '<replacement>' [flags]
subs_filter '<regex>'   '<replacement>' [flags]
```

| Flag | Meaning |
| --- | --- |
| `g` | 替换每一处，而非仅第一处 |
| `i` | 大小写不敏感（仅 `subs_filter`） |

`subs_filter` 的替换使用 [`regex`](https://docs.rs/regex/latest/regex/#syntax) crate 语法，捕获组写作 `$1`、`$2` 或 `${name}`。

## 示例

```toml
[plugins.rewriteLinks]
category = "sub_filter"
path = "^/docs"
status_codes = "200"
filters = [
    "sub_filter  'http://old.example.com' 'https://new.example.com' g",
    "subs_filter '<title>(.*?)</title>' '<title>$1 — Docs</title>' i",
    "sub_filter  '</head>' '<script src=\"/analytics.js\"></script></head>'",
]

[locations.docs]
upstream = "docs"
path = "/docs"
plugins = ["rewriteLinks"]
```

过滤器按列表顺序运行，每一条作用于前一条的输出。

## 行为

插件生效时会移除 `Content-Length`，改为 `Transfer-Encoding: chunked`，缓冲整段正文，在流结束时应用过滤器并输出结果。

### 压缩响应

插件可识别 `Content-Encoding: gzip`（含 `x-gzip`）、`Content-Encoding: deflate`（zlib 封装；解不开时回退尝试 raw deflate）、`Content-Encoding: br`（brotli）与 `Content-Encoding: zstd`。对这类响应，插件先解压正文，在明文上运行过滤器，再用相同算法重新压缩后输出：

- 保留 `Content-Encoding`（以到达时的算法重新压缩），删除 `Content-Length`，响应改为 `Transfer-Encoding: chunked`。
- 强 `ETag` 弱化为 `W/…`（它描述的是上游原始正文）；同理删除 `Content-MD5` 与 `Digest`；`Vary` 追加 `Accept-Encoding`（重压缩后的正文与上游发给其他 `Accept-Encoding` 的内容不同）。
- **上限：解压后明文 8 MiB**（几 MiB 的 gzip 可膨胀到 GB 级，仅限制原始字节挡不住压缩炸弹）。解压采用有界读取——解压器在膨胀过程中即按上限截停——超限的流会中途放弃，而不是先整体膨胀进内存再检查。超限、解压失败或重压缩失败时，原样透传压缩字节并保留 `Content-Encoding`。
- **无规则命中：** 逐字节输出上游原始压缩字节——正文字节级一致，不做重压缩。但该响应已被插件接管，头部仍会按上述规范化处理（chunked、弱化 `ETag`、`Vary: Accept-Encoding` 等）。

插件无法往返处理的编码——多值（如 `gzip, br`）或其他未知值——响应完全不改动（正文与响应头均原样透传），并记录一条 debug 日志。

### 代理层解压在先

代理可能在插件链运行**之前**就已解压上游的 gzip 响应：它会替未发送 `Accept-Encoding` 的客户端注入 `Accept-Encoding: gzip`（`[basic] upstream_accept_encoding`，默认开启），并为这些客户端撤销该编码。此时 `sub_filter` 看到的已是明文，与响应头是否仍写着 gzip 无关——代理的已解压标记会让插件走明文分支，正文不会被二次解压。

## 使用说明

- **整段响应体先缓冲到内存**再替换。请用 `path` 与 `status_codes` 收窄范围，远离大文件或流式端点。
- 即使最终没有任何规则命中，已被插件接管的响应也会转为 chunked 传输编码并弱化 `ETag`。这是流式设计的已知取舍：最终长度要到流结束才可知。
- 解析失败的规则是启动错误，`pingap -t` 可捕获引号问题。模式与替换须用单引号包裹，且自身不能含单引号。
- 替换在原始字节上进行；跨多字节 UTF-8 边界的正则匹配由正则引擎处理，但字面模式必须与正文中完全一致。
