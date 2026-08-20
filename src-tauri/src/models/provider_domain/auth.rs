use crate::models::{is_full_api_key_value, Provider};

pub fn has_api_key(provider: &Provider) -> bool {
    is_full_api_key_value(&provider.auth.api_key)
}

pub fn has_access_token(provider: &Provider) -> bool {
    !provider.auth.access_token.trim().is_empty()
}

pub fn has_session(provider: &Provider) -> bool {
    !provider.auth.session_cookie.trim().is_empty()
}

pub fn has_api_user(provider: &Provider) -> bool {
    !provider.auth.api_user.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderInput, ProviderProtocol};

    #[test]
    fn redacted_api_key_is_not_treated_as_usable_authentication() {
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Api;
        input.auth.api_key = "sk-abcd********wxyz".to_string();
        let provider = Provider::from_input(input, "provider-test".to_string());

        assert!(!has_api_key(&provider));
    }
}
