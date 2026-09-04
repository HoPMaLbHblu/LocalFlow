//! Wake-phrase detection. OWNER: audio agent. Placeholder with the agreed interface.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeMatch {
    /// The text doesn't start with the wake phrase.
    No,
    /// Only the wake phrase was said: arm and wait for the command.
    Armed,
    /// The wake phrase and a command in one breath: `rest` is the command.
    Command(String),
}

pub fn match_wake(_text: &str, _wake_phrase: &str) -> WakeMatch {
    WakeMatch::No
}
