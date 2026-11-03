//! Compose a managed block, then use the ordinary editor, diff and transaction.
use super::{errors::internal, native_support, ConfigurationService};
use crate::{models::*, services::agent_cli::config_support::text_document};
use std::sync::Arc;

const PREFIX: &str = "<!-- balancehub:instructions begin:";
const END: &str = "<!-- balancehub:instructions end -->";

fn rejected(message: &str) -> AgentConfigurationError {
    native_support::error(AgentConfigurationErrorKind::InvalidRequest, message)
}

fn compose(
    original: &str,
    scheme: &AgentInstructionSet,
    kind: AgentCliKind,
    remove: bool,
) -> Result<(String, bool), AgentConfigurationError> {
    let body = text_document::body(original);
    let start = body.find(PREFIX);
    let end = body.find(END);
    let (range, modified) = match (start, end) {
        (None, None) => (None, false),
        (Some(start), Some(end)) if end > start => {
            let header_end = body[start..]
                .find(" -->")
                .map(|offset| start + offset + 4)
                .ok_or_else(|| rejected("共享指令标记不完整，请先在原生配置编辑器中修复"))?;
            if header_end > end
                || body[header_end..].contains(PREFIX)
                || body[end + END.len()..].contains(END)
            {
                return Err(rejected(
                    "文件包含多处共享指令标记，请先在原生配置编辑器中合并",
                ));
            }
            let header = &body[start + PREFIX.len()..header_end - 4];
            let expected = header
                .rsplit_once(':')
                .map(|(_, hash)| hash)
                .ok_or_else(|| rejected("共享指令标记无效"))?;
            let current = body[header_end..end]
                .trim_matches(['\r', '\n'])
                .replace("\r\n", "\n");
            let modified = super::service::digest(current.as_bytes()) != expected;
            (Some(start..end + END.len()), modified)
        }
        _ => return Err(rejected("共享指令标记不完整，请先在原生配置编辑器中修复")),
    };
    if remove && range.is_none() {
        return Err(rejected("此文件没有 BalanceHub 共享指令"));
    }
    let content = [
        scheme.common.trim_matches(['\r', '\n']),
        scheme
            .additions
            .get(&kind)
            .map(String::as_str)
            .unwrap_or_default()
            .trim_matches(['\r', '\n']),
    ]
    .into_iter()
    .filter(|text| !text.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
    .replace("\r\n", "\n");
    if !remove && content.trim().is_empty() {
        return Err(rejected("此 Agent 的公共正文与补充正文均为空"));
    }
    let block = if remove {
        String::new()
    } else {
        format!(
            "{PREFIX}{}:{} -->\n{content}\n{END}",
            scheme.id,
            super::service::digest(content.as_bytes())
        )
    };
    let output = if let Some(range) = range {
        let mut text = body.to_owned();
        text.replace_range(range, &block);
        text
    } else if body.is_empty() {
        format!("{block}\n")
    } else {
        format!(
            "{body}{}{block}\n",
            if body.ends_with("\n\n") {
                ""
            } else if body.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            }
        )
    };
    Ok((text_document::restore_layout(original, &output), modified))
}

impl ConfigurationService {
    pub(crate) fn instruction_targets(
        &self,
        actor: &str,
        request: AgentConfigurationListRequest,
        settings: &AppSettings,
    ) -> Result<Vec<AgentConfigurationSource>, AgentConfigurationError> {
        let workspace = request.workspace.is_some();
        let mut sources = self
            .list_sources(actor, request, Some(settings))?
            .sources
            .into_iter()
            .filter(|source| source.format == AgentConfigurationFormat::Markdown)
            .collect::<Vec<_>>();
        sources.sort_by_key(|source| {
            let name = std::path::Path::new(&source.path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let preferred = match name {
                "AGENTS.override.md" if !source.revision.is_missing => 0,
                "AGENTS.md" | "CLAUDE.md" | "GEMINI.md" | "GROK.md" => 1,
                "AGENTS.override.md" => 3,
                _ => 2,
            };
            (
                workspace && source.scope != AgentAssetScope::Workspace,
                preferred,
                source.revision.is_missing,
            )
        });
        Ok(sources)
    }

    pub(crate) fn begin_instruction_edit(
        &self,
        actor: &str,
        request: AgentInstructionEditRequest,
        scheme: &AgentInstructionSet,
    ) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
        let anchor = self.source(actor, &request.source)?;
        let source = Arc::clone(&anchor.authority);
        if source.spec.format != AgentConfigurationFormat::Markdown {
            return Err(rejected("请选择 Agent 原生指令文件"));
        }
        super::service::require_edit(&source)?;
        let original = std::str::from_utf8(source.file.bytes().unwrap_or_default())
            .map_err(|_| rejected("指令文件不是 UTF-8 文本"))?;
        let (text, modified) = compose(original, scheme, source.source.agent_kind, request.remove)?;
        let mut edit = self.admit_edit(actor, source.source.agent_kind, vec![(source, text)])?;
        if modified {
            let diagnostic = native_support::diagnostic(
                "sharedInstructionsModified",
                "共享区块已在本地修改。请在差异预览中合并需要保留的内容后再保存。",
                AgentConfigurationDiagnosticSeverity::Warning,
            );
            edit.diagnostics.push(diagnostic.clone());
            let mut state = self.state.lock().map_err(|_| internal())?;
            if let Some(record) = state.edits.get_mut(&edit.edit_id).and_then(Arc::get_mut) {
                record.public.diagnostics.push(diagnostic);
            }
        }
        Ok(edit)
    }
}
