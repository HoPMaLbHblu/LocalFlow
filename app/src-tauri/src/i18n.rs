//! The few texts the Rust side shows itself: the tray menu and notifications.
//! Everything else is translated in the frontend (app/src/i18n).

/// Languages the app is translated into.
pub const LANGUAGES: [&str; 3] = ["en", "ru", "de"];

pub struct Texts {
    pub open: &'static str,
    pub run: &'static str,
    pub quit: &'static str,
    /// `{name}` is replaced with the automation's name.
    pub failed: &'static str,
    pub finished: &'static str,
    pub unknown_error: &'static str,
    /// `{version}`: the new version.
    pub update_title: &'static str,
    /// `{current}`: the installed version.
    pub update_body: &'static str,
    /// Tray menu: voice control.
    pub voice_mute: &'static str,
    pub voice_unmute: &'static str,
    pub stop_all: &'static str,
    /// Tray tooltip, by voice state.
    pub tip_idle: &'static str,
    pub tip_listening: &'static str,
    pub tip_processing: &'static str,
    pub tip_speaking: &'static str,
    pub tip_muted: &'static str,
    pub tip_error: &'static str,
}

const EN: Texts = Texts {
    open: "Open LocalFlow",
    run: "Run",
    quit: "Quit",
    failed: "{name} failed",
    finished: "{name} finished",
    unknown_error: "unknown error",
    update_title: "LocalFlow {version} is available",
    update_body: "You have {current}. Open LocalFlow to download the new version.",
    voice_mute: "Voice: mute",
    voice_unmute: "Voice: unmute",
    stop_all: "Stop all running automations",
    tip_idle: "LocalFlow: voice ready",
    tip_listening: "LocalFlow: listening",
    tip_processing: "LocalFlow: processing",
    tip_speaking: "LocalFlow: speaking",
    tip_muted: "LocalFlow: voice muted",
    tip_error: "LocalFlow: voice problem",
};

const RU: Texts = Texts {
    open: "Открыть LocalFlow",
    run: "Запустить",
    quit: "Выход",
    failed: "«{name}»: ошибка",
    finished: "«{name}» выполнено",
    unknown_error: "неизвестная ошибка",
    update_title: "Вышла LocalFlow {version}",
    update_body: "У вас {current}. Откройте LocalFlow, чтобы скачать новую версию.",
    voice_mute: "Голос: выключить микрофон",
    voice_unmute: "Голос: включить микрофон",
    stop_all: "Остановить все запущенные сценарии",
    tip_idle: "LocalFlow: голос готов",
    tip_listening: "LocalFlow: слушаю",
    tip_processing: "LocalFlow: обрабатываю",
    tip_speaking: "LocalFlow: говорю",
    tip_muted: "LocalFlow: голос отключён",
    tip_error: "LocalFlow: проблема с голосом",
};

const DE: Texts = Texts {
    open: "LocalFlow öffnen",
    run: "Ausführen",
    quit: "Beenden",
    failed: "„{name}“ fehlgeschlagen",
    finished: "„{name}“ abgeschlossen",
    unknown_error: "unbekannter Fehler",
    update_title: "LocalFlow {version} ist verfügbar",
    update_body: "Du hast {current}. Öffne LocalFlow, um die neue Version herunterzuladen.",
    voice_mute: "Sprache: stummschalten",
    voice_unmute: "Sprache: Stummschaltung aufheben",
    stop_all: "Alle laufenden Automatisierungen stoppen",
    tip_idle: "LocalFlow: Sprache bereit",
    tip_listening: "LocalFlow: hört zu",
    tip_processing: "LocalFlow: verarbeitet",
    tip_speaking: "LocalFlow: spricht",
    tip_muted: "LocalFlow: Sprache stummgeschaltet",
    tip_error: "LocalFlow: Problem mit der Sprache",
};

pub fn texts(language: &str) -> &'static Texts {
    match language {
        "ru" => &RU,
        "de" => &DE,
        _ => &EN,
    }
}

/// Turn a setting ("auto", "en", "ru", "de") into a supported language code.
pub fn resolve(setting: &str) -> &'static str {
    let wanted = if setting == "auto" || setting.is_empty() {
        sys_locale::get_locale().unwrap_or_default()
    } else {
        setting.to_string()
    };
    let prefix = wanted.split(['-', '_']).next().unwrap_or("").to_lowercase();
    LANGUAGES.into_iter().find(|l| *l == prefix).unwrap_or("en")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_languages() {
        assert_eq!(resolve("ru"), "ru");
        assert_eq!(resolve("de-AT"), "de");
        assert_eq!(resolve("fr"), "en");
        assert!(LANGUAGES.contains(&resolve("auto")));
        assert_eq!(texts("de").quit, "Beenden");
    }
}
