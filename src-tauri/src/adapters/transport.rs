use crate::{
    models::{AppSettings, Provider},
    network,
};
use reqwest::Client;

pub(crate) const USER_AGENT_VALUE: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/131.0.0.0 Safari/537.36"
);

pub(crate) fn build_client(settings: &AppSettings, provider: &Provider) -> Result<Client, String> {
    network::build_provider_client(settings, provider)
}
