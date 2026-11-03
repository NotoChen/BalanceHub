use crate::models::{OAuthCredential, OAuthVendor};
use serde::{Deserialize, Serialize};

/// WebDAV carries account identity only. Keeping a separate wire type prevents
/// imported sync documents from installing credentials or taking over renewal.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PortableOAuthIdentity {
    pub vendor: OAuthVendor,
    account_id: String,
    subject: String,
    email: String,
    organization_id: String,
    organization_name: String,
    project_id: String,
    plan: String,
    source: String,
}

impl PortableOAuthIdentity {
    pub(super) fn matches(&self, local: &OAuthCredential) -> bool {
        self.vendor == local.vendor
            && self.account_id == local.account_id
            && self.subject == local.subject
            && self.email == local.email
            && self.organization_id == local.organization_id
            && self.project_id == local.project_id
    }

    pub(super) fn with_local_session(self, local: OAuthCredential) -> OAuthCredential {
        OAuthCredential {
            organization_name: self.organization_name,
            plan: self.plan,
            source: self.source,
            ..local
        }
    }
}

impl From<&OAuthCredential> for PortableOAuthIdentity {
    fn from(local: &OAuthCredential) -> Self {
        Self {
            vendor: local.vendor,
            account_id: local.account_id.clone(),
            subject: local.subject.clone(),
            email: local.email.clone(),
            organization_id: local.organization_id.clone(),
            organization_name: local.organization_name.clone(),
            project_id: local.project_id.clone(),
            plan: local.plan.clone(),
            source: local.source.clone(),
        }
    }
}

impl From<PortableOAuthIdentity> for OAuthCredential {
    fn from(remote: PortableOAuthIdentity) -> Self {
        Self {
            account_id: remote.account_id,
            subject: remote.subject,
            email: remote.email,
            organization_id: remote.organization_id,
            organization_name: remote.organization_name,
            project_id: remote.project_id,
            plan: remote.plan,
            source: remote.source,
            ..Self::new(remote.vendor)
        }
    }
}
