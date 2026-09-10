//! Normalising text and parsing it into intents (English, Russian, German).
//!
//! The grammar is a small closed vocabulary kept in data tables per language (`EN`, `RU`, `DE`):
//! add a phrase there and it works. Nothing here derives a shell command or any free-form action
//! from speech: a sentence is either one of the known intents or `Intent::Unknown`. The only
//! free text that survives parsing is the *name* after a "run"/"stop" verb, and the controller
//! only ever matches it against the names of existing automations.
//!
//! All table entries are written in *normalised* form (see [`normalize`]): lower case, no
//! punctuation, `ё`->`е`, `ß`->`ss`, umlauts folded (`ü`->`u`), so "führe" is written "fuhre".

use super::{SettingChange, VoiceSettings};

/// The three languages voice understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
    De,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::De => "de",
        }
    }
    pub fn from_code(code: &str) -> Option<Lang> {
        match code.trim().to_lowercase().split(['-', '_']).next().unwrap_or("") {
            "en" => Some(Lang::En),
            "ru" => Some(Lang::Ru),
            "de" => Some(Lang::De),
            _ => None,
        }
    }
}

/// What a sentence means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Start an automation; `target` is the spoken name (may be empty: "run").
    Run { target: String },
    /// Stop a run; `None` = "stop" on its own.
    Stop { target: Option<String> },
    StopAll,
    WhatsRunning,
    ListCommands,
    ListAutomations,
    /// A whitelisted app setting.
    Setting(SettingChange),
    /// Voice control's own switches.
    StopListening,
    Mute,
    Unmute,
    SpokenFeedback(bool),
    Yes,
    No,
    Unknown,
}

/// Longest sentence (in words) that is considered at all.
pub const MAX_WORDS: usize = 24;

// ---- normalising -------------------------------------------------------------------------------

/// Lower-case, strip punctuation and accent quirks (ё->е, ß->ss, ä/ö/ü->a/o/u), collapse spaces.
/// Apostrophes vanish ("don't" -> "dont"); every other symbol (punctuation, emoji) is a space.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        let folded: &str = match c {
            'ё' => "е",
            'ß' => "ss",
            'ä' | 'á' | 'à' | 'â' => "a",
            'ö' | 'ó' | 'ò' | 'ô' => "o",
            'ü' | 'ú' | 'ù' | 'û' => "u",
            'é' | 'è' | 'ê' => "e",
            '\'' | '’' | '`' => continue,
            '\u{300}'..='\u{36f}' => continue,
            _ => "",
        };
        if !folded.is_empty() {
            push_word_char(&mut out, &mut pending_space, folded);
        } else if c.is_alphanumeric() {
            let mut buf = [0u8; 4];
            push_word_char(&mut out, &mut pending_space, c.encode_utf8(&mut buf));
        } else {
            pending_space = true;
        }
    }
    out
}

fn push_word_char(out: &mut String, pending_space: &mut bool, s: &str) {
    if *pending_space && !out.is_empty() {
        out.push(' ');
    }
    *pending_space = false;
    out.push_str(s);
}

/// Normalised words.
pub fn words(text: &str) -> Vec<String> {
    normalize(text).split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect()
}

// ---- language detection ------------------------------------------------------------------------

const DE_MARKERS: &[&str] = &[
    "starte", "starten", "fuhre", "fuhr", "bitte", "ja", "nein", "hilfe", "was", "kann", "kannst", "ich", "sagen", "befehle", "stopp", "stoppe",
    "alles", "alle", "automatisierung", "automatisierungen", "gerade", "lauft", "zeige", "meine", "liste", "schalte", "sprache", "dunkles", "dunkler",
    "helles", "heller", "benachrichtigungen", "bestatigen", "bestatige", "abbrechen", "stumm", "ausfuhren", "mikrofon", "und", "nicht", "die", "das",
    "der", "ein", "aus", "welche", "mit", "stummschalten", "jawohl", "danke",
];

/// "en", "ru" or "de" when the text shows it; `None` for text without letters.
/// Cyrillic means Russian; German umlauts or common German words mean German; other Latin text
/// is English (the app's default).
pub fn detect_language(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if lower.chars().any(|c| ('\u{400}'..='\u{4ff}').contains(&c)) {
        return Some("ru");
    }
    if !lower.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    if lower.chars().any(|c| matches!(c, 'ä' | 'ö' | 'ü' | 'ß')) {
        return Some("de");
    }
    let ws = words(&lower);
    let hits = ws.iter().filter(|w| DE_MARKERS.contains(&w.as_str())).count();
    // Two words of a four-word sentence ("was kann ich sagen") or a single strong word.
    if hits >= 2 || (hits == 1 && ws.len() <= 2 && !ws.iter().any(|w| matches!(w.as_str(), "an" | "ein" | "aus" | "die" | "das" | "der" | "was" | "und" | "mit"))) {
        return Some("de");
    }
    Some("en")
}

// ---- vocabulary tables -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Simple {
    Yes,
    No,
    WhatsRunning,
    ListCommands,
    ListAutomations,
    StopListening,
    Mute,
    Unmute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subject {
    Notifications,
    UpdateCheck,
    Dota,
    Autostart,
    Feedback,
    ThemeDark,
    ThemeLight,
    ThemeSystem,
    Language(&'static str),
}

struct Vocab {
    /// Whole sentences.
    simple: &'static [(&'static str, Simple)],
    /// Words that start "run <name>" (several words allowed).
    run: &'static [&'static str],
    /// "<name> starten": the verb comes last.
    run_suffix: &'static [&'static str],
    /// Words that start "stop <name>".
    stop: &'static [&'static str],
    stop_suffix: &'static [&'static str],
    /// "everything" in "stop everything".
    all: &'static [&'static str],
    /// Words dropped between the verb and the name ("run the automation X").
    nouns: &'static [&'static str],
    subjects: &'static [(&'static str, Subject)],
    on: &'static [&'static str],
    off: &'static [&'static str],
    /// Harmless words in a settings sentence.
    glue: &'static [&'static str],
    /// Words that show a language change is meant ("language", "switch to").
    lang_context: &'static [&'static str],
    /// Said first and ignored: "can you", "hey".
    polite: &'static [&'static str],
    /// Ignored at the very start or end.
    fillers: &'static [&'static str],
    /// Ignored at the end only.
    trailing: &'static [&'static str],
}

const EN: Vocab = Vocab {
    simple: &[
        ("yes", Simple::Yes),
        ("yeah", Simple::Yes),
        ("yep", Simple::Yes),
        ("yup", Simple::Yes),
        ("sure", Simple::Yes),
        ("confirm", Simple::Yes),
        ("confirmed", Simple::Yes),
        ("do it", Simple::Yes),
        ("go ahead", Simple::Yes),
        ("proceed", Simple::Yes),
        ("affirmative", Simple::Yes),
        ("yes confirm", Simple::Yes),
        ("yes do it", Simple::Yes),
        ("yes go ahead", Simple::Yes),
        ("no", Simple::No),
        ("nope", Simple::No),
        ("nah", Simple::No),
        ("dont", Simple::No),
        ("do not", Simple::No),
        ("dont do it", Simple::No),
        ("negative", Simple::No),
        ("cancel that", Simple::No),
        ("never mind", Simple::No),
        ("nevermind", Simple::No),
        ("no thanks", Simple::No),
        ("no dont", Simple::No),
        ("whats running", Simple::WhatsRunning),
        ("what is running", Simple::WhatsRunning),
        ("what is running now", Simple::WhatsRunning),
        ("whats running now", Simple::WhatsRunning),
        ("what is running right now", Simple::WhatsRunning),
        ("whats running right now", Simple::WhatsRunning),
        ("what is active", Simple::WhatsRunning),
        ("show running", Simple::WhatsRunning),
        ("status", Simple::WhatsRunning),
        ("what can i say", Simple::ListCommands),
        ("what can you do", Simple::ListCommands),
        ("help", Simple::ListCommands),
        ("list commands", Simple::ListCommands),
        ("show commands", Simple::ListCommands),
        ("commands", Simple::ListCommands),
        ("what commands are there", Simple::ListCommands),
        ("list automations", Simple::ListAutomations),
        ("list my automations", Simple::ListAutomations),
        ("show automations", Simple::ListAutomations),
        ("show my automations", Simple::ListAutomations),
        ("my automations", Simple::ListAutomations),
        ("what automations do i have", Simple::ListAutomations),
        ("what automations are there", Simple::ListAutomations),
        ("stop listening", Simple::StopListening),
        ("stop voice control", Simple::StopListening),
        ("stop voice", Simple::StopListening),
        ("turn off voice control", Simple::StopListening),
        ("turn off voice", Simple::StopListening),
        ("disable voice control", Simple::StopListening),
        ("go to sleep", Simple::StopListening),
        ("mute", Simple::Mute),
        ("mute microphone", Simple::Mute),
        ("mute mic", Simple::Mute),
        ("mute voice", Simple::Mute),
        ("unmute", Simple::Unmute),
        ("unmute microphone", Simple::Unmute),
        ("unmute mic", Simple::Unmute),
        ("unmute voice", Simple::Unmute),
    ],
    run: &["run", "start", "launch", "execute"],
    run_suffix: &[],
    stop: &["stop", "cancel", "abort", "halt"],
    stop_suffix: &[],
    all: &["everything", "all", "everything running", "all running", "all automations", "all of them", "it all"],
    nouns: &["the", "automation", "my"],
    subjects: &[
        ("notifications", Subject::Notifications),
        ("notification", Subject::Notifications),
        ("update check", Subject::UpdateCheck),
        ("update checks", Subject::UpdateCheck),
        ("update checking", Subject::UpdateCheck),
        ("check for updates", Subject::UpdateCheck),
        ("checking for updates", Subject::UpdateCheck),
        ("automatic updates", Subject::UpdateCheck),
        ("auto updates", Subject::UpdateCheck),
        ("updates", Subject::UpdateCheck),
        ("dota live helper", Subject::Dota),
        ("dota helper", Subject::Dota),
        ("dota live", Subject::Dota),
        ("dota live assistant", Subject::Dota),
        ("live match helper", Subject::Dota),
        ("autostart", Subject::Autostart),
        ("auto start", Subject::Autostart),
        ("start with windows", Subject::Autostart),
        ("start on boot", Subject::Autostart),
        ("start at startup", Subject::Autostart),
        ("launch at startup", Subject::Autostart),
        ("launch on startup", Subject::Autostart),
        ("run at startup", Subject::Autostart),
        ("spoken feedback", Subject::Feedback),
        ("voice feedback", Subject::Feedback),
        ("spoken replies", Subject::Feedback),
        ("speak replies", Subject::Feedback),
        ("spoken answers", Subject::Feedback),
        ("dark mode", Subject::ThemeDark),
        ("dark theme", Subject::ThemeDark),
        ("theme dark", Subject::ThemeDark),
        ("dark", Subject::ThemeDark),
        ("light mode", Subject::ThemeLight),
        ("light theme", Subject::ThemeLight),
        ("theme light", Subject::ThemeLight),
        ("light", Subject::ThemeLight),
        ("system theme", Subject::ThemeSystem),
        ("system mode", Subject::ThemeSystem),
        ("theme system", Subject::ThemeSystem),
        ("english", Subject::Language("en")),
        ("russian", Subject::Language("ru")),
        ("german", Subject::Language("de")),
        ("automatic", Subject::Language("auto")),
        ("auto", Subject::Language("auto")),
    ],
    on: &["on", "enable", "enabled", "activate"],
    off: &["off", "disable", "disabled", "deactivate"],
    glue: &["turn", "switch", "set", "change", "the", "a", "to", "my", "mode", "use", "make", "it", "theme", "language", "speak", "and", "now"],
    lang_context: &["language", "speak", "switch", "change"],
    polite: &["can you", "could you", "would you", "will you", "you can", "lets", "i want to", "i would like to", "i need to", "i want you to"],
    fillers: &["please", "pls", "plz", "ok", "okay", "hey", "hi", "hello", "localflow"],
    trailing: &["now", "thanks", "for me"],
};

const RU: Vocab = Vocab {
    simple: &[
        ("да", Simple::Yes),
        ("ага", Simple::Yes),
        ("угу", Simple::Yes),
        ("конечно", Simple::Yes),
        ("подтверждаю", Simple::Yes),
        ("подтверди", Simple::Yes),
        ("подтвердить", Simple::Yes),
        ("давай", Simple::Yes),
        ("делай", Simple::Yes),
        ("выполняй", Simple::Yes),
        ("да подтверждаю", Simple::Yes),
        ("да конечно", Simple::Yes),
        ("да давай", Simple::Yes),
        ("нет", Simple::No),
        ("неа", Simple::No),
        ("не надо", Simple::No),
        ("не нужно", Simple::No),
        ("не делай", Simple::No),
        ("не хочу", Simple::No),
        ("отбой", Simple::No),
        ("отмена", Simple::No),
        ("нет не надо", Simple::No),
        ("что запущено", Simple::WhatsRunning),
        ("что работает", Simple::WhatsRunning),
        ("что сейчас запущено", Simple::WhatsRunning),
        ("что сейчас работает", Simple::WhatsRunning),
        ("что выполняется", Simple::WhatsRunning),
        ("что сейчас выполняется", Simple::WhatsRunning),
        ("статус", Simple::WhatsRunning),
        ("что ты умеешь", Simple::ListCommands),
        ("что умеешь", Simple::ListCommands),
        ("что ты можешь", Simple::ListCommands),
        ("что можно сказать", Simple::ListCommands),
        ("помощь", Simple::ListCommands),
        ("помоги", Simple::ListCommands),
        ("справка", Simple::ListCommands),
        ("команды", Simple::ListCommands),
        ("список команд", Simple::ListCommands),
        ("какие команды", Simple::ListCommands),
        ("список автоматизаций", Simple::ListAutomations),
        ("мои автоматизации", Simple::ListAutomations),
        ("покажи автоматизации", Simple::ListAutomations),
        ("покажи мои автоматизации", Simple::ListAutomations),
        ("какие есть автоматизации", Simple::ListAutomations),
        ("какие у меня автоматизации", Simple::ListAutomations),
        ("перечисли автоматизации", Simple::ListAutomations),
        ("автоматизации", Simple::ListAutomations),
        ("перестань слушать", Simple::StopListening),
        ("хватит слушать", Simple::StopListening),
        ("не слушай", Simple::StopListening),
        ("выключи голосовое управление", Simple::StopListening),
        ("отключи голосовое управление", Simple::StopListening),
        ("отключи микрофон", Simple::Mute),
        ("выключи микрофон", Simple::Mute),
        ("заглуши", Simple::Mute),
        ("заглушить", Simple::Mute),
        ("микрофон выкл", Simple::Mute),
        ("включи микрофон", Simple::Unmute),
        ("включить микрофон", Simple::Unmute),
        ("микрофон включить", Simple::Unmute),
        ("микрофон вкл", Simple::Unmute),
    ],
    run: &["запусти автоматизацию", "включи автоматизацию", "включить автоматизацию", "запусти", "запустить", "запускай", "выполни", "выполнить", "стартуй"],
    run_suffix: &[],
    stop: &["стоп", "останови", "остановить", "отмени", "отменить", "прекрати", "прекратить", "заверши"],
    stop_suffix: &[],
    all: &["все", "всё", "всех", "все автоматизации", "все запущенные", "всё запущенное", "все сразу"],
    nouns: &["автоматизацию", "автоматизация", "автоматизации", "мою"],
    subjects: &[
        ("уведомления", Subject::Notifications),
        ("уведомление", Subject::Notifications),
        ("оповещения", Subject::Notifications),
        ("проверка обновлений", Subject::UpdateCheck),
        ("проверку обновлений", Subject::UpdateCheck),
        ("проверки обновлений", Subject::UpdateCheck),
        ("обновления", Subject::UpdateCheck),
        ("автообновление", Subject::UpdateCheck),
        ("автообновления", Subject::UpdateCheck),
        ("дота помощник", Subject::Dota),
        ("помощник доты", Subject::Dota),
        ("dota помощник", Subject::Dota),
        ("помощник dota", Subject::Dota),
        ("дота лайв", Subject::Dota),
        ("автозапуск", Subject::Autostart),
        ("автозапуска", Subject::Autostart),
        ("автозагрузка", Subject::Autostart),
        ("автозагрузку", Subject::Autostart),
        ("запуск при старте", Subject::Autostart),
        ("запуск при загрузке", Subject::Autostart),
        ("запуск вместе с windows", Subject::Autostart),
        ("запуск с windows", Subject::Autostart),
        ("запуск вместе с системой", Subject::Autostart),
        ("озвучивание", Subject::Feedback),
        ("озвучку", Subject::Feedback),
        ("голосовые ответы", Subject::Feedback),
        ("голосовой ответ", Subject::Feedback),
        ("голосовую обратную связь", Subject::Feedback),
        ("темная тема", Subject::ThemeDark),
        ("темную тему", Subject::ThemeDark),
        ("тема темная", Subject::ThemeDark),
        ("темный режим", Subject::ThemeDark),
        ("светлая тема", Subject::ThemeLight),
        ("светлую тему", Subject::ThemeLight),
        ("тема светлая", Subject::ThemeLight),
        ("светлый режим", Subject::ThemeLight),
        ("системная тема", Subject::ThemeSystem),
        ("системную тему", Subject::ThemeSystem),
        ("тема системная", Subject::ThemeSystem),
        ("английский", Subject::Language("en")),
        ("английском", Subject::Language("en")),
        ("русский", Subject::Language("ru")),
        ("русском", Subject::Language("ru")),
        ("немецкий", Subject::Language("de")),
        ("немецком", Subject::Language("de")),
        ("автоматический", Subject::Language("auto")),
        ("автоматически", Subject::Language("auto")),
        ("авто", Subject::Language("auto")),
    ],
    on: &["включи", "включить", "включай", "включено", "вкл", "активируй"],
    off: &["выключи", "выключить", "отключи", "отключить", "выкл", "откл", "деактивируй"],
    glue: &["на", "мне", "режим", "язык", "языка", "говори", "смени", "измени", "переключи", "переключись", "поставь", "установи", "сделай", "в", "тему"],
    lang_context: &["язык", "языка", "говори", "смени", "измени", "переключи", "переключись"],
    polite: &["можешь", "можете", "ты можешь", "я хочу", "хочу"],
    fillers: &["пожалуйста", "пжлст", "пожалуста", "эй"],
    trailing: &["сейчас", "спасибо", "для меня"],
};

const DE: Vocab = Vocab {
    simple: &[
        ("ja", Simple::Yes),
        ("jawohl", Simple::Yes),
        ("klar", Simple::Yes),
        ("sicher", Simple::Yes),
        ("bestatigen", Simple::Yes),
        ("bestatige", Simple::Yes),
        ("bestatigt", Simple::Yes),
        ("mach es", Simple::Yes),
        ("mach das", Simple::Yes),
        ("los", Simple::Yes),
        ("ja bestatigen", Simple::Yes),
        ("ja klar", Simple::Yes),
        ("nein", Simple::No),
        ("nee", Simple::No),
        ("nicht", Simple::No),
        ("lieber nicht", Simple::No),
        ("nicht ausfuhren", Simple::No),
        ("nein danke", Simple::No),
        ("vergiss es", Simple::No),
        ("auf keinen fall", Simple::No),
        ("abbruch", Simple::No),
        ("was lauft", Simple::WhatsRunning),
        ("was lauft gerade", Simple::WhatsRunning),
        ("was lauft jetzt", Simple::WhatsRunning),
        ("was lauft im moment", Simple::WhatsRunning),
        ("was ist aktiv", Simple::WhatsRunning),
        ("was wird ausgefuhrt", Simple::WhatsRunning),
        ("was wird gerade ausgefuhrt", Simple::WhatsRunning),
        ("hilfe", Simple::ListCommands),
        ("was kann ich sagen", Simple::ListCommands),
        ("was kannst du", Simple::ListCommands),
        ("was kannst du tun", Simple::ListCommands),
        ("befehle", Simple::ListCommands),
        ("kommandos", Simple::ListCommands),
        ("liste befehle", Simple::ListCommands),
        ("zeige befehle", Simple::ListCommands),
        ("welche befehle gibt es", Simple::ListCommands),
        ("liste automatisierungen", Simple::ListAutomations),
        ("meine automatisierungen", Simple::ListAutomations),
        ("zeige automatisierungen", Simple::ListAutomations),
        ("zeige meine automatisierungen", Simple::ListAutomations),
        ("automatisierungen", Simple::ListAutomations),
        ("welche automatisierungen gibt es", Simple::ListAutomations),
        ("welche automatisierungen habe ich", Simple::ListAutomations),
        ("hor auf zuzuhoren", Simple::StopListening),
        ("hor auf zu horen", Simple::StopListening),
        ("nicht mehr zuhoren", Simple::StopListening),
        ("zuhoren beenden", Simple::StopListening),
        ("stopp zuhoren", Simple::StopListening),
        ("sprachsteuerung aus", Simple::StopListening),
        ("sprachsteuerung ausschalten", Simple::StopListening),
        ("schalte sprachsteuerung aus", Simple::StopListening),
        ("stumm", Simple::Mute),
        ("stummschalten", Simple::Mute),
        ("mikrofon stumm", Simple::Mute),
        ("mikrofon stummschalten", Simple::Mute),
        ("mikrofon aus", Simple::Mute),
        ("mikrofon ausschalten", Simple::Mute),
        ("schalte mikrofon aus", Simple::Mute),
        ("stummschaltung aufheben", Simple::Unmute),
        ("mikrofon an", Simple::Unmute),
        ("mikrofon ein", Simple::Unmute),
        ("mikrofon einschalten", Simple::Unmute),
        ("schalte mikrofon ein", Simple::Unmute),
        ("ton an", Simple::Unmute),
    ],
    run: &["starte automatisierung", "starte", "starten", "fuhre", "fuhr"],
    run_suffix: &["starten", "ausfuhren"],
    stop: &["stopp", "stop", "stoppe", "stoppen", "abbrechen", "breche", "brich", "beende", "beenden"],
    stop_suffix: &["stoppen", "abbrechen", "beenden"],
    all: &["alles", "alle", "alle automatisierungen", "alles laufende", "alles zusammen"],
    nouns: &["die", "automatisierung", "meine", "den", "das"],
    subjects: &[
        ("benachrichtigungen", Subject::Notifications),
        ("benachrichtigung", Subject::Notifications),
        ("mitteilungen", Subject::Notifications),
        ("update prufung", Subject::UpdateCheck),
        ("updateprufung", Subject::UpdateCheck),
        ("updates prufen", Subject::UpdateCheck),
        ("automatische updates", Subject::UpdateCheck),
        ("automatische aktualisierungen", Subject::UpdateCheck),
        ("aktualisierungen", Subject::UpdateCheck),
        ("aktualisierungsprufung", Subject::UpdateCheck),
        ("dota live helfer", Subject::Dota),
        ("dota helfer", Subject::Dota),
        ("dota live hilfe", Subject::Dota),
        ("autostart", Subject::Autostart),
        ("mit windows starten", Subject::Autostart),
        ("start mit windows", Subject::Autostart),
        ("beim systemstart starten", Subject::Autostart),
        ("systemstart", Subject::Autostart),
        ("automatischer start", Subject::Autostart),
        ("sprachausgabe", Subject::Feedback),
        ("gesprochene antworten", Subject::Feedback),
        ("sprachruckmeldung", Subject::Feedback),
        ("dunkles design", Subject::ThemeDark),
        ("dunkler modus", Subject::ThemeDark),
        ("dunkel modus", Subject::ThemeDark),
        ("dunkles theme", Subject::ThemeDark),
        ("dunkelmodus", Subject::ThemeDark),
        ("design dunkel", Subject::ThemeDark),
        ("dunkel", Subject::ThemeDark),
        ("helles design", Subject::ThemeLight),
        ("heller modus", Subject::ThemeLight),
        ("helles theme", Subject::ThemeLight),
        ("hellmodus", Subject::ThemeLight),
        ("design hell", Subject::ThemeLight),
        ("hell", Subject::ThemeLight),
        ("systemdesign", Subject::ThemeSystem),
        ("system design", Subject::ThemeSystem),
        ("design system", Subject::ThemeSystem),
        ("systemmodus", Subject::ThemeSystem),
        ("englisch", Subject::Language("en")),
        ("russisch", Subject::Language("ru")),
        ("deutsch", Subject::Language("de")),
        ("automatisch", Subject::Language("auto")),
    ],
    on: &["an", "ein", "einschalten", "aktiviere", "aktivieren", "anschalten"],
    off: &["aus", "ausschalten", "deaktiviere", "deaktivieren", "abschalten", "ausstellen"],
    glue: &["schalte", "stelle", "wechsle", "wechseln", "auf", "die", "den", "das", "der", "mal", "sprache", "sprich", "zu", "modus", "mir", "um", "nur"],
    lang_context: &["sprache", "sprich", "wechsle", "wechseln", "stelle"],
    polite: &["kannst du", "konntest du", "ich mochte", "ich will", "kannst du mir"],
    fillers: &["bitte", "hallo"],
    trailing: &["jetzt", "danke", "fur mich"],
};

const VOCABS: [&Vocab; 3] = [&EN, &RU, &DE];

// ---- parsing -----------------------------------------------------------------------------------

/// Parse an utterance. Anything outside the grammar is `Intent::Unknown`.
pub fn parse(text: &str, settings: &VoiceSettings) -> Intent {
    let mut toks = words(text);
    strip_fillers(&mut toks, &words(&settings.wake_phrase));
    if toks.is_empty() || toks.len() > MAX_WORDS {
        return Intent::Unknown;
    }
    let sentence = toks.join(" ");
    for v in VOCABS {
        if let Some((_, kind)) = v.simple.iter().find(|(p, _)| *p == sentence) {
            return match kind {
                Simple::Yes => Intent::Yes,
                Simple::No => Intent::No,
                Simple::WhatsRunning => Intent::WhatsRunning,
                Simple::ListCommands => Intent::ListCommands,
                Simple::ListAutomations => Intent::ListAutomations,
                Simple::StopListening => Intent::StopListening,
                Simple::Mute => Intent::Mute,
                Simple::Unmute => Intent::Unmute,
            };
        }
    }
    if let Some(intent) = parse_setting(&toks) {
        return intent;
    }
    if let Some(intent) = parse_stop(&toks) {
        return intent;
    }
    if let Some(intent) = parse_run(&toks) {
        return intent;
    }
    Intent::Unknown
}

/// Is this (normalised) phrase a built-in voice command? Spoken names must not collide with it.
pub fn is_reserved_phrase(normalized: &str) -> bool {
    parse(normalized, &VoiceSettings { wake_phrase: String::new(), ..VoiceSettings::default() }) != Intent::Unknown
}

fn phrase_words(p: &str) -> Vec<&str> {
    p.split(' ').collect()
}

fn starts_with_phrase(toks: &[String], phrase: &str) -> Option<usize> {
    let p = phrase_words(phrase);
    (toks.len() >= p.len() && toks.iter().zip(&p).all(|(a, b)| a == b)).then_some(p.len())
}

fn strip_fillers(toks: &mut Vec<String>, wake: &[String]) {
    loop {
        if !wake.is_empty() && toks.len() >= wake.len() && toks.iter().zip(wake).all(|(a, b)| a == b) {
            toks.drain(..wake.len());
            continue;
        }
        let mut hit = None;
        for v in VOCABS {
            if let Some(n) = v.polite.iter().filter_map(|p| starts_with_phrase(toks, p)).max() {
                hit = Some(n);
            } else if v.fillers.iter().any(|f| toks.first().map(String::as_str) == Some(*f)) {
                hit = Some(1);
            }
            if hit.is_some() {
                break;
            }
        }
        match hit {
            Some(n) if n < toks.len() || toks.len() == 1 => {
                toks.drain(..n);
            }
            _ => break,
        }
    }
    loop {
        let mut cut = 0;
        for v in VOCABS {
            for f in v.fillers.iter().chain(v.trailing) {
                let p = phrase_words(f);
                if toks.len() > p.len() && toks[toks.len() - p.len()..].iter().zip(&p).all(|(a, b)| a == b) {
                    cut = cut.max(p.len());
                }
            }
        }
        if cut == 0 {
            break;
        }
        let keep = toks.len() - cut;
        toks.truncate(keep);
    }
}

fn parse_setting(toks: &[String]) -> Option<Intent> {
    if toks.len() > 10 {
        return None;
    }
    let mut best: Option<(usize, usize, Subject)> = None;
    for v in VOCABS {
        for (phrase, subject) in v.subjects {
            let p = phrase_words(phrase);
            if toks.len() < p.len() {
                continue;
            }
            for start in 0..=toks.len() - p.len() {
                if toks[start..start + p.len()].iter().zip(&p).all(|(a, b)| a == b) && best.map_or(true, |(_, len, _)| p.len() > len) {
                    best = Some((start, p.len(), *subject));
                }
            }
        }
    }
    let (start, len, subject) = best?;
    let (mut on, mut off, mut context) = (false, false, false);
    for (i, t) in toks.iter().enumerate() {
        if i >= start && i < start + len {
            continue;
        }
        let t = t.as_str();
        if VOCABS.iter().any(|v| v.on.contains(&t)) {
            on = true;
        } else if VOCABS.iter().any(|v| v.off.contains(&t)) {
            off = true;
        } else if VOCABS.iter().any(|v| v.glue.contains(&t)) {
            if VOCABS.iter().any(|v| v.lang_context.contains(&t)) {
                context = true;
            }
        } else {
            return None;
        }
    }
    if on && off {
        return None;
    }
    let state = on.then_some(true).or(off.then_some(false));
    let theme = |t: &str| Some(Intent::Setting(SettingChange::Theme(t.into())));
    match subject {
        Subject::Notifications => state.map(|s| Intent::Setting(SettingChange::Notifications(s))),
        Subject::UpdateCheck => state.map(|s| Intent::Setting(SettingChange::UpdateCheck(s))),
        Subject::Dota => state.map(|s| Intent::Setting(SettingChange::DotaLiveHelper(s))),
        Subject::Autostart => state.map(|s| Intent::Setting(SettingChange::Autostart(s))),
        Subject::Feedback => state.map(Intent::SpokenFeedback),
        Subject::ThemeDark => theme(if state == Some(false) { "light" } else { "dark" }),
        Subject::ThemeLight => theme(if state == Some(false) { "dark" } else { "light" }),
        Subject::ThemeSystem => {
            if state == Some(false) {
                None
            } else {
                theme("system")
            }
        }
        Subject::Language(code) => (state.is_none() && context).then(|| Intent::Setting(SettingChange::Language(code.into()))),
    }
}

fn strip_nouns(rest: &mut Vec<String>) {
    let original = rest.clone();
    while rest.first().is_some_and(|w| VOCABS.iter().any(|v| v.nouns.contains(&w.as_str()))) {
        rest.remove(0);
    }
    if rest.is_empty() && original.iter().any(|w| !VOCABS.iter().any(|v| v.nouns.contains(&w.as_str()))) {
        *rest = original;
    }
}

fn longest_prefix<'a>(toks: &[String], phrases: impl Iterator<Item = &'a &'a str>) -> Option<(usize, &'a str)> {
    phrases.filter_map(|p| starts_with_phrase(toks, p).map(|n| (n, *p))).max_by_key(|(n, _)| *n)
}

fn is_all(rest: &[String]) -> bool {
    let joined = rest.join(" ");
    VOCABS.iter().any(|v| v.all.contains(&joined.as_str()))
}

fn parse_stop(toks: &[String]) -> Option<Intent> {
    let mut rest: Vec<String>;
    let mut verb = "";
    if let Some((n, v)) = longest_prefix(toks, VOCABS.iter().flat_map(|v| v.stop.iter())) {
        verb = v;
        rest = toks[n..].to_vec();
    } else if toks.len() >= 2 && VOCABS.iter().any(|v| v.stop_suffix.contains(&toks[toks.len() - 1].as_str())) {
        rest = toks[..toks.len() - 1].to_vec();
    } else {
        return None;
    }
    if matches!(verb, "breche" | "brich") && rest.last().is_some_and(|w| w == "ab") {
        rest.pop();
    }
    strip_nouns(&mut rest);
    if rest.is_empty() {
        return Some(Intent::Stop { target: None });
    }
    if is_all(&rest) {
        return Some(Intent::StopAll);
    }
    Some(Intent::Stop { target: Some(rest.join(" ")) })
}

fn parse_run(toks: &[String]) -> Option<Intent> {
    let mut rest: Vec<String>;
    let mut verb = "";
    if let Some((n, v)) = longest_prefix(toks, VOCABS.iter().flat_map(|v| v.run.iter())) {
        verb = v;
        rest = toks[n..].to_vec();
    } else if toks.len() >= 2 && VOCABS.iter().any(|v| v.run_suffix.contains(&toks[toks.len() - 1].as_str())) {
        rest = toks[..toks.len() - 1].to_vec();
    } else {
        return None;
    }
    if matches!(verb, "fuhre" | "fuhr") && rest.last().is_some_and(|w| w == "aus") {
        rest.pop();
    }
    strip_nouns(&mut rest);
    Some(Intent::Run { target: rest.join(" ") })
}

// ---- tests ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Intent {
        parse(text, &VoiceSettings::default())
    }
    fn run(t: &str) -> Intent {
        Intent::Run { target: t.into() }
    }
    fn stop(t: &str) -> Intent {
        Intent::Stop { target: Some(t.into()) }
    }
    fn set(c: SettingChange) -> Intent {
        Intent::Setting(c)
    }

    #[test]
    fn normalizing() {
        assert_eq!(normalize("  Hello,   WORLD!! "), "hello world");
        assert_eq!(normalize("Тёмная тема"), "темная тема");
        assert_eq!(normalize("Führe die Straße aus"), "fuhre die strasse aus");
        assert_eq!(normalize("don't"), "dont");
        assert_eq!(normalize("a-b_c/d"), "a b c d");
        assert_eq!(normalize("🙂 ok 🙂"), "ok");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn run_in_three_languages() {
        assert_eq!(p("Run backup notes"), run("backup notes"));
        assert_eq!(p("please start the Zip backup"), run("zip backup"));
        assert_eq!(p("launch automation backup"), run("backup"));
        assert_eq!(p("hey, can you run backup?"), run("backup"));
        assert_eq!(p("Запусти бэкап"), run("бэкап"));
        assert_eq!(p("запустить фото"), run("фото"));
        assert_eq!(p("Включи автоматизацию бэкап заметок"), run("бэкап заметок"));
        assert_eq!(p("запусти автоматизацию уборка"), run("уборка"));
        assert_eq!(p("Starte Foto Sicherung"), run("foto sicherung"));
        assert_eq!(p("Führe Backup aus"), run("backup"));
        assert_eq!(p("starte die Automatisierung Aufräumen bitte"), run("aufraumen"));
        assert_eq!(p("Backup starten"), run("backup"));
        assert_eq!(p("run"), run(""));
    }

    #[test]
    fn stop_variants() {
        assert_eq!(p("stop"), Intent::Stop { target: None });
        assert_eq!(p("Cancel"), Intent::Stop { target: None });
        assert_eq!(p("abort"), Intent::Stop { target: None });
        assert_eq!(p("stop backup"), stop("backup"));
        assert_eq!(p("стоп"), Intent::Stop { target: None });
        assert_eq!(p("Останови бэкап"), stop("бэкап"));
        assert_eq!(p("отмени"), Intent::Stop { target: None });
        assert_eq!(p("Stopp"), Intent::Stop { target: None });
        assert_eq!(p("abbrechen"), Intent::Stop { target: None });
        assert_eq!(p("Stoppe Backup"), stop("backup"));
        assert_eq!(p("breche Backup ab"), stop("backup"));
        assert_eq!(p("Backup stoppen"), stop("backup"));
        assert_eq!(p("stop everything"), Intent::StopAll);
        assert_eq!(p("stop all"), Intent::StopAll);
        assert_eq!(p("останови всё"), Intent::StopAll);
        assert_eq!(p("стоп все"), Intent::StopAll);
        assert_eq!(p("stoppe alles"), Intent::StopAll);
        assert_eq!(p("alles stoppen"), Intent::StopAll);
    }

    #[test]
    fn information_and_voice_controls() {
        for t in ["what's running", "What is running now?", "что запущено", "Что сейчас работает", "Was läuft gerade", "was läuft"] {
            assert_eq!(p(t), Intent::WhatsRunning, "{t}");
        }
        for t in ["what can I say", "help", "list commands", "что ты умеешь", "помощь", "Was kann ich sagen", "Hilfe"] {
            assert_eq!(p(t), Intent::ListCommands, "{t}");
        }
        for t in ["list automations", "покажи автоматизации", "Zeige Automatisierungen"] {
            assert_eq!(p(t), Intent::ListAutomations, "{t}");
        }
        assert_eq!(p("stop listening"), Intent::StopListening);
        assert_eq!(p("перестань слушать"), Intent::StopListening);
        assert_eq!(p("hör auf zuzuhören"), Intent::StopListening);
        assert_eq!(p("mute"), Intent::Mute);
        assert_eq!(p("Unmute"), Intent::Unmute);
        assert_eq!(p("выключи микрофон"), Intent::Mute);
        assert_eq!(p("включи микрофон"), Intent::Unmute);
        assert_eq!(p("Mikrofon stumm"), Intent::Mute);
        assert_eq!(p("spoken feedback on"), Intent::SpokenFeedback(true));
        assert_eq!(p("turn off voice feedback"), Intent::SpokenFeedback(false));
        assert_eq!(p("выключи озвучивание"), Intent::SpokenFeedback(false));
        assert_eq!(p("Sprachausgabe an"), Intent::SpokenFeedback(true));
    }

    #[test]
    fn yes_and_no() {
        for t in ["yes", "Yes, confirm", "do it", "да", "подтверждаю", "ja", "bestätigen", "yes please"] {
            assert_eq!(p(t), Intent::Yes, "{t}");
        }
        for t in ["no", "No!", "don't", "нет", "nein", "no thanks", "не надо"] {
            assert_eq!(p(t), Intent::No, "{t}");
        }
    }

    #[test]
    fn whitelisted_settings() {
        use SettingChange::*;
        assert_eq!(p("notifications on"), set(Notifications(true)));
        assert_eq!(p("turn off notifications"), set(Notifications(false)));
        assert_eq!(p("Включи уведомления"), set(Notifications(true)));
        assert_eq!(p("выключи уведомления"), set(Notifications(false)));
        assert_eq!(p("Benachrichtigungen aus"), set(Notifications(false)));
        assert_eq!(p("schalte Benachrichtigungen ein"), set(Notifications(true)));
        assert_eq!(p("dark mode"), set(Theme("dark".into())));
        assert_eq!(p("switch to light theme"), set(Theme("light".into())));
        assert_eq!(p("system theme"), set(Theme("system".into())));
        assert_eq!(p("тёмная тема"), set(Theme("dark".into())));
        assert_eq!(p("включи светлую тему"), set(Theme("light".into())));
        assert_eq!(p("dunkles Design"), set(Theme("dark".into())));
        assert_eq!(p("helles Design"), set(Theme("light".into())));
        assert_eq!(p("turn off dark mode"), set(Theme("light".into())));
        assert_eq!(p("update check off"), set(UpdateCheck(false)));
        assert_eq!(p("enable automatic updates"), set(UpdateCheck(true)));
        assert_eq!(p("включи проверку обновлений"), set(UpdateCheck(true)));
        assert_eq!(p("Updateprüfung aus"), set(UpdateCheck(false)));
        assert_eq!(p("Dota live helper on"), set(DotaLiveHelper(true)));
        assert_eq!(p("выключи помощник доты"), set(DotaLiveHelper(false)));
        assert_eq!(p("autostart on"), set(Autostart(true)));
        assert_eq!(p("turn off autostart"), set(Autostart(false)));
        assert_eq!(p("включи автозапуск"), set(Autostart(true)));
        assert_eq!(p("Autostart ausschalten"), set(Autostart(false)));
        assert_eq!(p("language english"), set(Language("en".into())));
        assert_eq!(p("switch to russian"), set(Language("ru".into())));
        assert_eq!(p("смени язык на немецкий"), set(Language("de".into())));
        assert_eq!(p("язык русский"), set(Language("ru".into())));
        assert_eq!(p("Sprache Deutsch"), set(Language("de".into())));
        assert_eq!(p("language auto"), set(Language("auto".into())));
    }

    #[test]
    fn nothing_outside_the_grammar_is_understood() {
        for t in [
            "open the shell",
            "delete everything",
            "change allowed folders",
            "turn on system control",
            "disable confirmations",
            "allow system control",
            "format the disk",
            "run powershell",
            "turn on the lights",
            "english",
            "notifications",
            "autostart",
            "",
            "ok",
            "🙂",
            "включи систему управления",
            "отключи подтверждения",
            "lösche alles",
            "schalte die Bestätigung aus",
        ] {
            match p(t) {
                Intent::Unknown => {}
                // "run powershell" is a Run request for an automation with that name; it never executes anything itself.
                Intent::Run { .. } if t == "run powershell" => {}
                other => panic!("{t:?} -> {other:?}"),
            }
        }
    }

    #[test]
    fn odd_input_never_panics() {
        let long = "run ".to_string() + &"x ".repeat(5000);
        assert_eq!(p(&long), Intent::Unknown);
        for t in ["שלום עולם", "مرحبا", "\0\u{1}\u{7f}", "ǅ İ ẞ", "\u{202e}run backup", "a\u{300}\u{300}"] {
            let _ = p(t);
        }
    }

    #[test]
    fn language_detection() {
        assert_eq!(detect_language("run backup"), Some("en"));
        assert_eq!(detect_language("запусти бэкап"), Some("ru"));
        assert_eq!(detect_language("Führe Backup aus"), Some("de"));
        assert_eq!(detect_language("was kann ich sagen"), Some("de"));
        assert_eq!(detect_language("hilfe"), Some("de"));
        assert_eq!(detect_language("🙂"), None);
    }

    #[test]
    fn reserved_phrases() {
        for r in ["stop", "помощь", "hilfe", "yes", "dark mode", "mute"] {
            assert!(is_reserved_phrase(r), "{r}");
        }
        for ok in ["backup", "my backup", "фото", "morning routine"] {
            assert!(!is_reserved_phrase(ok), "{ok}");
        }
    }
}
