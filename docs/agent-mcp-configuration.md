---
layout: default
title: MCP 配置覆盖范围
description: MCP 传输、各 Agent 的配置映射与跨 Agent 转换边界。
page_class: document-page
---

# MCP 配置覆盖范围

BalanceHub 管理 MCP 连接配置。配置文件能被解析和写入，不代表已经连接服务器、完成 OAuth 登录或验证服务端功能；这些由目标 Agent 执行。

## 传输方式

MCP 官方标准传输是 **stdio** 和 **Streamable HTTP**。旧版 **HTTP + SSE** 仍有客户端支持；Streamable HTTP 响应中使用 SSE，并不代表连接采用旧版 HTTP + SSE。自定义传输需要客户端具有对应实现。

下面是 BalanceHub 的配置解析与写入范围，依据 2026-09-28 核对的官方文档及 Gemini 官方源码：

| 目标 Agent | stdio | Streamable HTTP | 旧版 HTTP + SSE | WebSocket |
| --- | --- | --- | --- | --- |
| Claude Code | 支持 | 支持 | 支持 | 支持，写入 `type: "ws"` |
| Codex | 支持 | 支持 | 官方配置未提供直接映射 | 官方配置未提供映射 |
| Gemini CLI | 支持 | 支持 | 支持 | 当前连接实现未提供映射 |
| Grok Build | 支持 | 支持 | 支持 | 官方配置未提供映射 |

- Claude 接受 `http` / `streamable-http`；远程 URL 必须注明 `type`，不会将缺少 `type` 的条目猜成 SSE。
- Codex 使用 `mcp_servers`，根据 `command` / `url` 区分 stdio / HTTP。
- Gemini 支持文档中的 `httpUrl` / `url`，也解析明确标注 `type` 的 URL；写入继续使用文档明确说明的 `httpUrl` 表达 HTTP、`url` 表达 SSE。未标注类型的 `url` 按该文档的 SSE 语义处理；官方主分支已改为默认 HTTP，因此跨版本共享时应显式注明 `type`。BalanceHub 写入 SSE 时也会显式注明类型。
- Grok 的配置参考明确列出 HTTP/SSE；显式传输声明优先，未声明类型且 URL 以 `/sse` 结尾时识别为 SSE。
- 其他自定义传输保留原文查看入口；未实现的映射会明确说明，不会改写为 HTTP。

扫描、共享收录、配置预览和连接差异使用同一个 MCP 解析器。原生文件的来源优先级、启停、权限和信任仍由各 Agent 的规则处理。

## 连接字段与格式差异

比较连接使用传输、命令、参数、URL、工作目录、环境变量、请求头及认证/执行依赖。JSON/TOML、字段顺序、HTTP 请求头名称大小写，以及 `headers` / `http_headers` 等等价表示不构成连接差异。

可转换的连接选项包括：

- 各 Agent 的 OAuth client ID、scope 列表，以及具有对应字段的回调端口或回调 URL；Claude 的空格分隔 scopes 与其他 Agent 的数组统一比较。
- Claude `headersHelper` 与 Codex `http_headers_helper`；命令原样作为配置保存，BalanceHub 不执行它。命令依赖来源 Agent 的专用变量时仍需调整。
- Codex/Grok 的 `bearer_token_env_var` 与支持环境变量展开的请求头。
- Codex `env_http_headers` 与其他 Agent 的请求头环境变量引用。
- Codex 本地 `env_vars` 与其他 Agent 的同名进程环境变量引用。

环境变量按引用转换，不读取本机变量值，不复制 OAuth 登录令牌。不同 Agent 的登录、凭据存储、默认回调路径、凭据过滤和命令执行环境由各自实现决定，配置后可能仍需在目标 Agent 登录。

有多个静态或动态凭据来源时，会保留其差异；无法保留优先级的转换会说明冲突。带默认值的变量、拼接表达式、URL/命令中的变量、远程执行环境、Gemini Google 凭据和服务账号模拟等，只在能够保持对应语义时转换。未映射的实际依赖会列出具体字段，不因 JSON/TOML 格式不同阻止配置。

更新同名 MCP 时，替换连接及其认证字段，保留目标的启停、超时、工具筛选、信任等本地策略；旧连接的认证字段不会残留并覆盖新连接。生成目标配置后会再次解析并核对连接内容，一旦转换造成内容变化就停止预览生成。

## 文档依据

- [MCP 标准传输](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports)
- [Claude Code MCP](https://code.claude.com/docs/en/mcp)
- [Codex MCP（OpenAI Docs）](https://developers.openai.com/codex/mcp) 与 [配置参考](https://developers.openai.com/codex/config-reference)
- [Gemini CLI MCP](https://geminicli.com/docs/tools/mcp-server/) 与 [官方连接实现](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/mcp-client.ts)
- [Grok Build MCP](https://docs.x.ai/build/features/mcp-servers) 与 [配置参考](https://docs.x.ai/build/settings/reference)

本范围说明记录配置层实现。Agent 配置功能按仓库规则由用户验收，不将构建通过视为实际连接验收。
