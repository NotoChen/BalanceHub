# Agent 能力矩阵

> 本文件由 `npm run docs:capabilities` 从
> `crates/agent/src/services/agent_cli/capabilities.snapshot.json` 生成，请勿手工编辑。

`✓` 表示该能力由 Rust 侧适配器提供，`—` 表示暂不支持。前端与文档使用同一份快照；能力变化必须先用
`UPDATE_AGENT_CAPABILITIES=1 npm run cargo -- test -p balancehub-agent --lib capability_matrix_matches_snapshot`
更新快照，再运行 `npm run docs:capabilities` 生成文档。

| Agent | CLI 命令 | 临时启动 | 模型选择 | 会话读取 | 会话搜索 | 会话详情 | 继续会话 | 会话命名 | 测活 | 默认配置 | 资产盘点 | Hook 盘点 | 运行观察 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Codex CLI（`codex`） | `codex` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| Claude Code（`claudeCode`） | `claude` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Gemini CLI（`gemini`） | `gemini` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| Grok Build（`grok`） | `grok` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| Qwen Code（`qwenCode`） | `qwen` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| OpenCode（`opencode`） | `opencode` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| Pi（`pi`） | `pi` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ |
| Crush（`crush`） | `crush` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| Copilot CLI（`copilot`） | `copilot` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ |
| Droid（`droid`） | `droid` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| Qoder（`qoder`） | `qodercli` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | ✓ |
| CodeBuddy（`codebuddy`） | `codebuddy` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| OpenClaw（`openclaw`） | `openclaw` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| jcode（`jcode`） | `jcode` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| MothX（`mothx`） | `mothx` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| Kimi Code（`kimiCode`） | `kimi` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| StepCode（`stepCode`） | `step` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| DevEco Code（`deveco`） | `deveco` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| MiMo Code（`mimoCode`） | `mimo` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| AtomCode（`atomcode`） | `atomcode` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| Cline（`cline`） | `cline` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| Prime Agent（`primeAgent`） | `prime-agent` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| PenguinHarness（`penguin`） | `penguin` | ✓ | — | ✓ | ✓ | ✓ | ✓ | — | — | ✓ | ✓ | — | ✓ |
| Goose（`goose`） | `goose` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ |
| Hermes Agent（`hermes`） | `hermes` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| dsh（`dsh`） | `dsh` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| omp（`omp`） | `omp` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| Mister Morph（`morph`） | `morph` | ✓ | ✓ | ✓ | ✓ | ✓ | — | — | ✓ | ✓ | ✓ | — | ✓ |
| MiniMax Code（`minimaxCode`） | `mcode` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| Raycast（`raycast`） | `raycast` | — | — | — | — | — | — | — | — | ✓ | ✓ | — | — |
| OmO（`omo`） | `omo` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ |
| Command Code（`commandCode`） | `command-code` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ |
