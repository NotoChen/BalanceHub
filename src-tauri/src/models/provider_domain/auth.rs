use crate::models::Provider;

pub fn has_api_key(provider: &Provider) -> bool {
    !provider.auth.api_key.trim().is_empty()
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
