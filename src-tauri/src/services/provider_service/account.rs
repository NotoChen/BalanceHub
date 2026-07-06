use crate::adapters::newapi::NewApiAdapter;

use super::{find_provider, ProviderService};

impl<'a> ProviderService<'a> {
    pub async fn change_password(
        &self,
        id: String,
        original_password: String,
        password: String,
    ) -> Result<String, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        NewApiAdapter
            .change_password(&data.settings, &provider, &original_password, &password)
            .await
    }
}
