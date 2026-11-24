//! Telemetry is disabled in Wisp.
//!
//! This crate keeps the `telemetry::event!` macro signature so call sites keep
//! compiling, but every event is discarded locally: nothing is queued, logged
//! to disk, or sent over the network.
pub use serde_json;
use std::collections::HashMap;

/// A telemetry event. Retained only so existing call sites compile; events are
/// never delivered anywhere.
#[derive(Clone, Debug, Default)]
pub struct Event {
    pub event_type: String,
    pub event_properties: HashMap<String, serde_json::Value>,
}

/// No-op telemetry event macro.
///
/// The property expressions are still borrowed so that variables used only for
/// telemetry don't trigger unused-variable warnings, but nothing is recorded.
///
/// ```
/// # let url = "https://example.com";
/// telemetry::event!("Keymap Changed", version = "1.0.0");
/// telemetry::event!("Documentation Viewed", url, source = "Extension Upsell");
/// ```
#[macro_export]
macro_rules! event {
    ($name:expr) => {{
        let _ = &$name;
    }};
    ($name:expr, $($key:ident $(= $value:expr)?),+ $(,)?) => {{
        let _ = &$name;
        $(
            let _ = &$crate::serialize_property!($key $(= $value)?);
        )+
    }};
}

#[macro_export]
macro_rules! serialize_property {
    ($key:ident) => {
        $key
    };
    ($key:ident = $value:expr) => {
        $value
    };
}

/// Discards the event. Telemetry is disabled in Wisp.
pub fn send_event(event: Event) {
    drop(event);
}
