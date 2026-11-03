//! 会话用量的费用估算：复用模型目录缓存。

use crate::models::SessionUsageSummary;

pub(crate) async fn estimate_session_usage(
    usage: SessionUsageSummary,
) -> Result<SessionUsageSummary, String> {
    if usage.models.len() > 256 {
        return Err("会话模型数量超出范围".to_owned());
    }
    let ids = usage
        .models
        .iter()
        .filter_map(|model| model.model.clone())
        .collect();
    let _ = crate::api::fetch_model_metadata(ids).await?;
    crate::state::core_state().model_catalog()?.estimate(usage)
}
