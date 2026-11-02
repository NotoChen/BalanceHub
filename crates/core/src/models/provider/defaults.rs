pub(super) fn quota_per_unit() -> f64 {
    crate::util::DEFAULT_QUOTA_PER_UNIT
}

pub(super) fn quota_display_type() -> String {
    "currency".to_string()
}

pub(super) fn currency_symbol() -> String {
    "$".to_string()
}

pub(super) fn currency_exchange_rate() -> f64 {
    1.0
}
