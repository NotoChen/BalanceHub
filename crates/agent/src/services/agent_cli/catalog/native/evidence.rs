//! Fixture identity for the historical distribution validation records.
//! These records do not control runtime configuration availability.
pub(crate) fn fixture_digest() -> String {
    super::super::digest(include_bytes!("distribution-fixture.json"))
}
