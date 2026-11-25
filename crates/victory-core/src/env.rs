//! Environment switches read by the shared modules.

/// The variable's value when it is set and is text.
pub fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}
