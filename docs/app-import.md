# 把中转站导出到其他应用

在中转站卡片的快捷操作中点击 **导出到其他应用**，可以把当前卡片的接口地址和 API Key 交给 Magpie 或 CC Switch。

- **Magpie**：直接打开中转站导入确认页；没有 API Key 时，可以在 Magpie 中补填。
- **CC Switch**：选择 Codex CLI、Claude Code、Gemini CLI、Grok Build、OpenCode、OpenClaw 或 Hermes，再确认导入。需要先为卡片设置 API Key。

传递的是卡片当前使用的调用 API Key，不包含账号密码、Cookie 或访问令牌。仅有脱敏 Key 时按未取得完整 Key 处理，可在 Magpie 中补填。添加和覆盖都由接收应用确认；BalanceHub 的“已请求打开”提示只表示已经提交打开请求。

## Magpie 的地址和模型

| 导入字段 | 使用的配置 |
| --- | --- |
| OpenAI Chat Completions | Grok 的专用接口地址；未设置时使用中转站地址，整理为 `/v1` 基址。 |
| OpenAI Responses | 仅在设置 Codex 专用接口地址时附带，整理为 `/v1` 基址。 |
| Anthropic Messages | 仅在设置 Claude Code 专用接口地址时附带，使用不带 `/v1` 的 API 根地址。 |
| Gemini | 仅在设置 Gemini CLI 专用接口地址时附带，整理为 `/v1beta` 基址。 |

专用接口地址位于中转站编辑窗口的高级配置中。导入保留地址的子路径，并处理已有的版本后缀和完整的模型列表接口路径。提供完整 OpenAI 接口路径时使用它的实际父级基址，包括无版本号或 `/v2` 的地址，不额外追加 `/v1`。

Magpie 会用导入的 API Key 获取模型。BalanceHub 的默认模型和账号可用模型列表不会作为 Magpie 的模型白名单传入，避免把其他分组的模型混入或只剩一个模型。

每张卡片使用稳定的 Magpie 标识，支持纯中文名称。修改名称或更换 Key 后再次导入同一卡片，Magpie 会对已有的导入项显示替换确认。

## 无法打开或导入时

- 先安装并手动启动一次目标应用，让系统登记其导入链接处理器。Windows 便携版只解压而未运行时可能尚未登记。
- 打开失败时，BalanceHub 会显示原因，并提供 **复制导入链接**。只有点击该按钮才会写入剪贴板。
- Magpie 的导入协议要求公网接口使用 HTTPS；`localhost`、`.local` 和私有／回环 IP 地址可以使用 HTTP。BalanceHub 会按这一要求显示具体原因。
- 接口地址不能包含 URL 账号密码、查询参数或锚点；API Key 应放在独立的凭据字段中。

macOS 优先按应用标识打开，再尝试系统链接处理器；Windows 使用原生 ShellExecuteExW，保留中文和百分号编码；Linux 使用 `gio open`／`xdg-open`。打开请求有超时，菜单和主界面不等待外部应用退出。

协议依据：Magpie [v0.1.1285 的导入解析器](https://github.com/yetone/magpie/blob/d783df46cbac9021e26a6bd71ca0b55544552eb2/internal/provider/importurl.go)及[官方导入说明](https://usemagpie.ai/docs/zh/import)，CC Switch 的 V1 Provider 深链协议。

## 手动验收

按仓库的 Agent 配置验收规则，真实导入由用户在目标应用中确认。

1. 卡片有 Key：打开 Magpie，核对名称、地址和当前 Key；确认前取消不应改变目标应用配置。
2. 卡片没有 Key：Magpie 入口仍可使用；CC Switch 分组显示需要设置 Key 的原因。
3. 使用纯中文名称，再改名或更换 Key 后重复导入：Magpie 对同一导入项显示替换确认。
4. 存在各 Agent 专用地址：检查协议、子路径和版本后缀；未设置的专用协议不应凭空出现。
5. CC Switch 各 Agent 入口保留对应地址和已设置的默认模型。
6. 未安装应用、系统打开失败或超时：菜单已收起，主界面仍可操作，忙碌提示结束；复制由用户点击触发。
7. 连续点击同一入口不重复发起正在进行的请求；重新发起后，旧失败窗口和过期结果不覆盖新操作；关闭失败窗口或 App 后不再出现过期提示。
