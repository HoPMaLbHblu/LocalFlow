//! Fuzzy matching of a spoken name against automation names and aliases.
//!
//! Like `remote::pick_automation` it never guesses between several close matches: when more
//! than one automation scores within [`AMBIGUITY_MARGIN`] of the best, the result is
//! `Match::Ambiguous` and the caller asks which one was meant.
//!
//! Stages:
//! 1. **Exact**: the normalised phrase equals a normalised name or alias. Two different
//!    automations behind the same phrase (duplicate names, an alias equal to another
//!    automation's name) is `Ambiguous`, never a silent pick.
//! 2. **Fuzzy**: each automation is scored 0..1 by its best name/alias (see [`score_phrase`]):
//!    word order does not matter, words may be partial ("tidy" for "Tidy screenshots") or
//!    misspelled by the recogniser (edit distance), "back up" equals "backup", and spoken
//!    numbers ("two") equal digits. Words containing digits must match exactly, so
//!    "backup 2" never matches "backup 3". Aliases get [`ALIAS_BONUS`].

use super::{grammar::words, AutomationInfo, VoiceAlias};

/// Below this score there is no match at all.
pub const MIN_SCORE: f32 = 0.62;
/// From here a single clear winner is `Likely` (run at once); between `MIN_SCORE` and this it is
/// `Weak` (the controller asks "did you mean ...?").
pub const LIKELY_SCORE: f32 = 0.78;
/// Several automations within this distance of the best score are "ambiguous".
pub const AMBIGUITY_MARGIN: f32 = 0.12;
/// Added to the score of an alias match: the user chose that phrase on purpose.
pub const ALIAS_BONUS: f32 = 0.05;
/// A word is only fuzzily compared when it is at least this long (short words must be exact).
pub const FUZZY_MIN_LEN: usize = 4;
/// Minimum similarity (1 - edit distance / length) for two words to count as the same word.
pub const WORD_SIMILARITY_MIN: f32 = 0.75;
/// The more lenient limits used only to suggest "did you mean" candidates (never to run).
pub const SUGGEST_FUZZY_MIN_LEN: usize = 3;
pub const SUGGEST_WORD_SIMILARITY_MIN: f32 = 0.5;
pub const SUGGEST_SCORE: f32 = 0.5;
/// Similarity of a word that is the beginning of another ("screen" for "screenshots").
pub const PREFIX_SIMILARITY: f32 = 0.9;
/// Whole-phrase similarity (spaces ignored) needed for "back up" ~ "backup" style matches.
pub const JOINED_SIMILARITY_MIN: f32 = 0.85;
/// At most this many candidates are returned.
pub const MAX_CANDIDATES: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: i64,
    /// The automation's name (also when it was matched through an alias).
    pub name: String,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Match {
    /// The normalised phrase is exactly one automation's name or alias.
    Exact(i64),
    /// One clear fuzzy winner.
    Likely(i64),
    /// One winner, but a weak one: ask before running.
    Weak(i64),
    /// Several automations fit about equally: ask which one.
    Ambiguous(Vec<Candidate>),
    None,
}

struct Entry {
    id: i64,
    name: String,
    phrase: String,
    alias: bool,
}

fn entries(automations: &[AutomationInfo], aliases: &[VoiceAlias]) -> Vec<Entry> {
    let mut out = Vec::new();
    for a in automations {
        let phrase = canonical(&a.name);
        if !phrase.is_empty() {
            out.push(Entry { id: a.id, name: a.name.clone(), phrase, alias: false });
        }
    }
    for al in aliases {
        // An alias of an automation that no longer exists is ignored.
        if let Some(a) = automations.iter().find(|a| a.id == al.automation_id) {
            let phrase = canonical(&al.phrase);
            if !phrase.is_empty() {
                out.push(Entry { id: a.id, name: a.name.clone(), phrase, alias: true });
            }
        }
    }
    out
}

const STOPWORDS: &[&str] = &["the", "a", "an", "my", "мою", "мой", "моя", "die", "der", "das", "den", "meine", "mein"];

/// Spoken numbers become digits so "backup two" matches "Backup 2".
const NUMBER_WORDS: &[(&str, &str)] = &[
    ("zero", "0"), ("one", "1"), ("two", "2"), ("three", "3"), ("four", "4"), ("five", "5"), ("six", "6"), ("seven", "7"), ("eight", "8"), ("nine", "9"), ("ten", "10"),
    ("ноль", "0"), ("один", "1"), ("одина", "1"), ("два", "2"), ("три", "3"), ("четыре", "4"), ("пять", "5"), ("шесть", "6"), ("семь", "7"), ("восемь", "8"), ("девять", "9"), ("десять", "10"),
    ("null", "0"), ("eins", "1"), ("zwei", "2"), ("drei", "3"), ("vier", "4"), ("funf", "5"), ("sechs", "6"), ("sieben", "7"), ("acht", "8"), ("neun", "9"), ("zehn", "10"),
];

/// Words with numbers as digits; a leading article ("the", "die") is dropped. Other stopwords
/// stay: "Sync a" and "Sync b" must remain different names.
fn tokens(text: &str) -> Vec<String> {
    let mut ws = words(text);
    while ws.len() > 1 && STOPWORDS.contains(&ws[0].as_str()) {
        ws.remove(0);
    }
    ws.into_iter().map(|w| NUMBER_WORDS.iter().find(|(n, _)| *n == w).map(|(_, d)| d.to_string()).unwrap_or(w)).collect()
}

/// Normalised words joined by one space, without stopwords and with numbers as digits.
fn canonical(text: &str) -> String {
    tokens(text).join(" ")
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn similarity(a: &str, b: &str) -> f32 {
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return 1.0;
    }
    1.0 - levenshtein(a, b) as f32 / max as f32
}

fn has_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// How well one spoken word matches one name word: 1 equal, 0 different.
fn word_similarity(wanted: &str, name: &str, lenient: bool) -> f32 {
    if wanted == name {
        return 1.0;
    }
    if has_digit(wanted) || has_digit(name) {
        return 0.0;
    }
    let (lw, ln) = (wanted.chars().count(), name.chars().count());
    if lw >= 3 && name.starts_with(wanted) {
        return PREFIX_SIMILARITY;
    }
    let min_len = if lenient { SUGGEST_FUZZY_MIN_LEN } else { FUZZY_MIN_LEN };
    if lw.min(ln) < min_len {
        return 0.0;
    }
    let sim = similarity(wanted, name);
    if sim >= if lenient { SUGGEST_WORD_SIMILARITY_MIN } else { WORD_SIMILARITY_MIN } {
        sim
    } else {
        0.0
    }
}

/// Score 0..1 of a spoken phrase against one name/alias phrase (both already canonical).
///
/// `score = wanted_coverage * (0.6 + 0.4 * phrase_coverage)` where wanted_coverage is the average
/// best word similarity of the spoken words and phrase_coverage that of the name's words. Saying
/// half of a two-word name therefore scores 0.8, all of it 1.0, and a spoken word that matches
/// nothing halves the score.
pub fn score_phrase(wanted: &str, phrase: &str, lenient: bool) -> f32 {
    let p: Vec<&str> = phrase.split(' ').filter(|s| !s.is_empty()).collect();
    // A filler word the recogniser added ("backup the photos") is ignored unless the name has it.
    let w: Vec<&str> = wanted.split(' ').filter(|s| !s.is_empty() && (!STOPWORDS.contains(s) || p.contains(s))).collect();
    if w.is_empty() || p.is_empty() {
        return 0.0;
    }
    let best = |from: &str, among: &[&str]| among.iter().map(|o| word_similarity(from, o, lenient)).fold(0.0f32, f32::max);
    let wanted_cov = w.iter().map(|x| best(x, &p)).sum::<f32>() / w.len() as f32;
    let phrase_cov = p.iter().map(|x| best(x, &w)).sum::<f32>() / p.len() as f32;
    let mut score = wanted_cov * (0.6 + 0.4 * phrase_cov);
    // "back up" ~ "backup": compare without spaces, but only when the numbers agree.
    let (jw, jp) = (w.concat(), p.concat());
    if (w.len() > 1 || p.len() > 1) && digits(&jw) == digits(&jp) && jw.chars().count().min(jp.chars().count()) >= 5 {
        let sim = similarity(&jw, &jp);
        if sim >= JOINED_SIMILARITY_MIN {
            score = score.max(sim * 0.97);
        }
    }
    score.min(1.0)
}

/// Match a spoken name. Never guesses between close matches.
pub fn find(wanted: &str, automations: &[AutomationInfo], aliases: &[VoiceAlias]) -> Match {
    let wanted = canonical(wanted);
    if wanted.is_empty() {
        return Match::None;
    }
    let entries = entries(automations, aliases);

    // 1. exact
    let mut exact: Vec<&Entry> = entries.iter().filter(|e| e.phrase == wanted).collect();
    if !exact.is_empty() {
        let mut ids: Vec<i64> = exact.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() == 1 {
            return Match::Exact(ids[0]);
        }
        exact.sort_by_key(|e| e.id);
        exact.dedup_by_key(|e| e.id);
        return Match::Ambiguous(exact.iter().take(MAX_CANDIDATES).map(|e| Candidate { id: e.id, name: e.name.clone(), score: 1.0 }).collect());
    }

    // 2. fuzzy
    let ranked = rank(&wanted, &entries, false);
    let ranked: Vec<Candidate> = ranked.into_iter().filter(|c| c.score >= MIN_SCORE).collect();
    let Some(top) = ranked.first() else { return Match::None };
    let rivals = ranked.iter().skip(1).filter(|c| c.score >= top.score - AMBIGUITY_MARGIN).count();
    if rivals > 0 {
        return Match::Ambiguous(ranked.into_iter().take(MAX_CANDIDATES).collect());
    }
    if top.score >= LIKELY_SCORE {
        Match::Likely(top.id)
    } else {
        Match::Weak(top.id)
    }
}

/// Best score per automation, highest first.
fn rank(wanted: &str, entries: &[Entry], lenient: bool) -> Vec<Candidate> {
    let mut best: Vec<Candidate> = Vec::new();
    for e in entries {
        let mut score = score_phrase(wanted, &e.phrase, lenient);
        if score > 0.0 && e.alias {
            score = (score + ALIAS_BONUS).min(1.0);
        }
        match best.iter_mut().find(|c| c.id == e.id) {
            Some(c) => {
                if score > c.score {
                    c.score = score;
                }
            }
            None => best.push(Candidate { id: e.id, name: e.name.clone(), score }),
        }
    }
    best.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then(a.id.cmp(&b.id)));
    best
}

/// "Did you mean" candidates for text that matched nothing: lenient scoring, at most `limit`.
/// Suggestions are only ever shown, never run.
pub fn suggest(text: &str, automations: &[AutomationInfo], aliases: &[VoiceAlias], limit: usize) -> Vec<Candidate> {
    let wanted = canonical(text);
    if wanted.is_empty() {
        return Vec::new();
    }
    rank(&wanted, &entries(automations, aliases), true).into_iter().filter(|c| c.score >= SUGGEST_SCORE).take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: i64, name: &str) -> AutomationInfo {
        AutomationInfo { id, name: name.into(), description: String::new(), enabled: true, allow_system: false }
    }
    fn alias(p: &str, id: i64) -> VoiceAlias {
        VoiceAlias { phrase: p.into(), automation_id: id, automation_name: String::new() }
    }
    fn ids(m: &Match) -> Vec<i64> {
        match m {
            Match::Ambiguous(c) => c.iter().map(|c| c.id).collect(),
            Match::Exact(i) | Match::Likely(i) | Match::Weak(i) => vec![*i],
            Match::None => vec![],
        }
    }

    fn sample() -> Vec<AutomationInfo> {
        vec![a(1, "Backup notes"), a(2, "Backup photos"), a(3, "Tidy screenshots"), a(4, "Morning routine")]
    }

    #[test]
    fn exact_ignores_case_punctuation_and_order_of_nothing_else() {
        let l = sample();
        assert_eq!(find("backup notes", &l, &[]), Match::Exact(1));
        assert_eq!(find("Backup, Notes!", &l, &[]), Match::Exact(1));
        assert_eq!(find("the tidy screenshots", &l, &[]), Match::Exact(3));
    }

    #[test]
    fn near_collisions_are_ambiguous_and_never_guessed() {
        let l = sample();
        let m = find("backup", &l, &[]);
        assert!(matches!(&m, Match::Ambiguous(c) if c.len() == 2), "{m:?}");
        // a typo that clearly favours one
        assert_eq!(find("bakup notes", &l, &[]), Match::Likely(1));
        assert_eq!(find("backup photo", &l, &[]), Match::Likely(2));
        // word order tolerant
        assert_eq!(find("notes backup", &l, &[]), Match::Likely(1));
    }

    #[test]
    fn partial_names_and_typos() {
        let l = sample();
        assert_eq!(find("tidy", &l, &[]), Match::Likely(3));
        assert_eq!(find("screenshots", &l, &[]), Match::Likely(3));
        assert_eq!(find("morning routin", &l, &[]), Match::Likely(4));
        assert_eq!(find("tidi screenshot", &l, &[]), Match::Likely(3));
        assert!(matches!(find("rutine", &l, &[]), Match::None | Match::Weak(4)));
        assert_eq!(find("something else entirely", &l, &[]), Match::None);
        assert_eq!(find("the", &l, &[]), Match::None);
        assert_eq!(find("", &l, &[]), Match::None);
    }

    #[test]
    fn digits_must_match_exactly() {
        let l = vec![a(1, "Backup 2"), a(2, "Backup 3")];
        assert_eq!(find("backup 2", &l, &[]), Match::Exact(1));
        assert_eq!(find("backup two", &l, &[]), Match::Exact(1));
        assert_eq!(find("backup drei", &l, &[]), Match::Exact(2));
        assert!(matches!(find("backup", &l, &[]), Match::Ambiguous(_)));
        assert_eq!(find("backup 4", &l, &[]), Match::None);
        let l = vec![a(1, "Photos 2024"), a(2, "Photos 2025")];
        assert_eq!(find("photos 2025", &l, &[]), Match::Exact(2));
        assert_eq!(find("photos 2023", &l, &[]), Match::None);
    }

    #[test]
    fn russian_and_german_names() {
        let l = vec![a(1, "Бэкап заметок"), a(2, "Бэкап фотографий"), a(3, "Aufräumen Downloads"), a(4, "Größe prüfen")];
        assert_eq!(find("бэкап заметок", &l, &[]), Match::Exact(1));
        assert!(matches!(find("бэкап", &l, &[]), Match::Ambiguous(_)));
        assert_eq!(find("бекап фотографии", &l, &[]), Match::Likely(2));
        assert_eq!(find("aufraumen downloads", &l, &[]), Match::Exact(3));
        assert_eq!(find("Aufräumen", &l, &[]), Match::Likely(3));
        assert_eq!(find("grosse prufen", &l, &[]), Match::Exact(4));
    }

    #[test]
    fn joined_words() {
        let l = vec![a(1, "Backup"), a(2, "Zip files")];
        assert_eq!(find("back up", &l, &[]), Match::Likely(1));
        assert_eq!(find("zipfiles", &l, &[]), Match::Likely(2));
    }

    #[test]
    fn aliases() {
        let l = sample();
        let al = vec![alias("nightly", 2), alias("Бэкап", 1)];
        assert_eq!(find("nightly", &l, &al), Match::Exact(2));
        assert_eq!(find("бэкап", &l, &al), Match::Exact(1));
        assert_eq!(find("nightle", &l, &al), Match::Likely(2));
        // an alias of a deleted automation is ignored
        assert_eq!(find("ghost", &l, &[alias("ghost", 99)]), Match::None);
        // the alias and the name of the same automation are one answer
        assert_eq!(find("backup notes", &l, &[alias("backup notes", 1)]), Match::Exact(1));
        // alias equal to another automation's name: never silently chosen
        let m = find("tidy screenshots", &l, &[alias("tidy screenshots", 1)]);
        assert_eq!(ids(&m), vec![1, 3]);
        assert!(matches!(m, Match::Ambiguous(_)));
        // duplicate names are ambiguous too
        let m = find("dup", &[a(1, "Dup"), a(2, "dup")], &[]);
        assert!(matches!(m, Match::Ambiguous(c) if c.len() == 2));
    }

    #[test]
    fn suggestions_are_lenient_but_bounded() {
        let l = sample();
        let s = suggest("tidy screenshot please", &l, &[], 3);
        assert_eq!(s.first().map(|c| c.id), Some(3));
        assert!(suggest("xyz", &l, &[], 3).is_empty());
        assert!(suggest("backup", &l, &[], 1).len() == 1);
    }

    #[test]
    fn thresholds_are_consistent() {
        assert!(MIN_SCORE < LIKELY_SCORE);
        assert!(AMBIGUITY_MARGIN > ALIAS_BONUS);
        assert!(SUGGEST_SCORE < MIN_SCORE);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
    }
}
