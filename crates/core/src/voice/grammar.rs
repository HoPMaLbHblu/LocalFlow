//! Normalising text and parsing it into intents (en/ru/de). OWNER: logic agent. Placeholder.

/// Lower-case, strip punctuation and accents-insensitive quirks (ё/е, ß/ss, umlauts), collapse spaces.
pub fn normalize(text: &str) -> String {
    text.trim().to_lowercase()
}
