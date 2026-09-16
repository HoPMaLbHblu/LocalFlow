//! Wake-phrase detection for always-on listening.
//!
//! The recogniser's text is compared with the wake phrase at the START of the utterance only: a
//! command that merely contains the phrase later in the sentence is not a wake-up. Matching works on
//! whole words (word boundaries are never split), ignores spaces ("hey local flow" = "hey localflow"),
//! tolerates small typos and a lost leading word ("local flow, run backup": the VAD often clips the
//! first word), and does not depend on the language: it only compares normalised letters.

use super::{grammar, matcher::levenshtein};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeMatch {
    /// The text doesn't start with the wake phrase.
    No,
    /// Only the wake phrase was said: arm and wait for the command.
    Armed,
    /// The wake phrase and a command in one breath: `rest` is the command.
    Command(String),
}

/// The wake phrase without its first word is accepted only if it has at least this many letters
/// (so "flow" alone never wakes anything).
const DROPPED_FIRST_WORD_MIN_CHARS: usize = 5;

/// How many typos (character edits) a phrase of `len` letters tolerates.
fn tolerance(len: usize) -> usize {
    match len {
        0..=4 => 0,
        5..=13 => 1,
        _ => 2,
    }
}

/// The best prefix of `words` that matches `target` (letters only, spaces ignored):
/// `(number of words used, edit distance)`.
fn best_prefix(words: &[String], target: &str) -> Option<(usize, usize)> {
    let squashed: String = target.chars().filter(|c| *c != ' ').collect();
    let target_len = squashed.chars().count();
    let tol = tolerance(target_len);
    let max_words = (target.split(' ').count() + 2).min(words.len());
    let mut best: Option<(usize, usize)> = None;
    let mut joined = String::new();
    for (i, w) in words.iter().take(max_words).enumerate() {
        joined.push_str(w);
        // A prefix far longer than the target cannot match any more.
        if joined.chars().count() > target_len + tol {
            break;
        }
        let d = levenshtein(&joined, &squashed);
        if d <= tol && best.map_or(true, |(_, bd)| d < bd) {
            best = Some((i + 1, d));
        }
    }
    best
}

pub fn match_wake(text: &str, wake_phrase: &str) -> WakeMatch {
    let phrase_words = grammar::words(wake_phrase);
    if phrase_words.is_empty() {
        return WakeMatch::No;
    }
    let words = grammar::words(text);
    if words.is_empty() {
        return WakeMatch::No;
    }
    let full = phrase_words.join(" ");
    let mut used = best_prefix(&words, &full).map(|(n, _)| n);
    if used.is_none() && phrase_words.len() >= 2 {
        let tail = phrase_words[1..].join(" ");
        if tail.chars().filter(|c| *c != ' ').count() >= DROPPED_FIRST_WORD_MIN_CHARS {
            used = best_prefix(&words, &tail).map(|(n, _)| n);
        }
    }
    match used {
        None => WakeMatch::No,
        Some(n) if n >= words.len() => WakeMatch::Armed,
        Some(n) => WakeMatch::Command(words[n..].join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(s: &str) -> WakeMatch {
        WakeMatch::Command(s.into())
    }

    #[test]
    fn exact_and_spacing_variants() {
        assert_eq!(match_wake("hey localflow", "hey localflow"), WakeMatch::Armed);
        assert_eq!(match_wake("Hey, Local Flow!", "hey localflow"), WakeMatch::Armed);
        assert_eq!(match_wake("hey localflow run backup", "hey localflow"), cmd("run backup"));
        assert_eq!(match_wake("hey local flow, run the backup.", "hey localflow"), cmd("run the backup"));
        assert_eq!(match_wake("hey localflow", "hey local flow"), WakeMatch::Armed);
    }

    #[test]
    fn dropped_leading_word() {
        assert_eq!(match_wake("localflow stop all", "hey localflow"), cmd("stop all"));
        assert_eq!(match_wake("local flow", "hey localflow"), WakeMatch::Armed);
        assert_eq!(match_wake("flow stop all", "hey localflow"), WakeMatch::No);
    }

    #[test]
    fn small_typos() {
        assert_eq!(match_wake("hey lokalflow run backup", "hey localflow"), cmd("run backup"));
        assert_eq!(match_wake("hey local flo", "hey localflow"), WakeMatch::Armed);
        assert_eq!(match_wake("hay localflow", "hey localflow"), WakeMatch::Armed);
    }

    #[test]
    fn false_accepts_are_refused() {
        assert_eq!(match_wake("hey look at the flow", "hey localflow"), WakeMatch::No);
        assert_eq!(match_wake("hello world", "hey localflow"), WakeMatch::No);
        assert_eq!(match_wake("hey there", "hey localflow"), WakeMatch::No);
        assert_eq!(match_wake("local news today", "hey localflow"), WakeMatch::No);
        assert_eq!(match_wake("flow", "hey localflow"), WakeMatch::No);
        assert_eq!(match_wake("they locally flowed", "hey localflow"), WakeMatch::No);
    }

    #[test]
    fn the_phrase_in_the_middle_is_not_a_wake_up() {
        assert_eq!(match_wake("please run backup hey localflow", "hey localflow"), WakeMatch::No);
        assert_eq!(match_wake("I said hey localflow to it", "hey localflow"), WakeMatch::No);
    }

    #[test]
    fn empty_input() {
        assert_eq!(match_wake("", "hey localflow"), WakeMatch::No);
        assert_eq!(match_wake("   ", "hey localflow"), WakeMatch::No);
        assert_eq!(match_wake("hey localflow", ""), WakeMatch::No);
        assert_eq!(match_wake("hey localflow", "!!!"), WakeMatch::No);
        assert_eq!(match_wake("???", "hey localflow"), WakeMatch::No);
    }

    #[test]
    fn russian_and_german() {
        assert_eq!(match_wake("Привет, локал флоу", "привет локалфлоу"), WakeMatch::Armed);
        assert_eq!(match_wake("привет локалфлоу запусти бэкап", "привет локалфлоу"), cmd("запусти бэкап"));
        assert_eq!(match_wake("локал флоу запусти бэкап", "привет локалфлоу"), cmd("запусти бэкап"));
        assert_eq!(match_wake("привет мир", "привет локалфлоу"), WakeMatch::No);
        assert_eq!(match_wake("Hallo Fluss starte Backup", "hallo localflow"), WakeMatch::No);
        assert_eq!(match_wake("Hallo, Lokalflow, starte das Backup", "hallo localflow"), cmd("starte das backup"));
        assert_eq!(match_wake("Höre zu Lokalflow", "höre zu localflow"), WakeMatch::Armed);
    }

    #[test]
    fn single_word_phrase() {
        assert_eq!(match_wake("jarvis open it", "jarvis"), cmd("open it"));
        assert_eq!(match_wake("jarvas", "jarvis"), WakeMatch::Armed);
        assert_eq!(match_wake("service open it", "jarvis"), WakeMatch::No);
        assert_eq!(match_wake("hi", "hey"), WakeMatch::No);
    }

    #[test]
    fn short_phrases_need_exact_words() {
        assert_eq!(match_wake("ok go", "ok go"), WakeMatch::Armed);
        assert_eq!(match_wake("ok no", "ok go"), WakeMatch::No);
    }
}
