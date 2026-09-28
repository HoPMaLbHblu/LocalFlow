//! Link sets: named tables of web addresses that open together in a browser, e.g. "Work"
//! with 40 tabs or "Morning news" with 3. Kept in `links.json` in LocalFlow's data folder
//! (the previous version is always kept as `links.bak`; deleted sets go to a trash list).
//!
//! ```lua
//! links.open("Work")                                   -- every link of the set, in its browser
//! links.open({ "https://example.com", "news.ycombinator.com" }, { browser = "chrome", new_window = true })
//! links.save("Morning", "https://a.com\nhttps://b.com") -- a list or pasted text
//! links.add("Morning", "https://c.com", "C")
//! links.get("Work")                                    -- { "https://...", ... }
//! links.list()                                         -- { { name = "Work", count = 42, browser = "chrome" }, ... }
//! links.import_bookmarks("Work tabs", "chrome", "Work") -- a bookmarks folder into a set
//! ```

use std::{path::PathBuf, time::Duration};

use mlua::{Lua, Table, Value};
use serde::{Deserialize, Serialize};

use crate::appdata;

pub const BROWSERS: &[&str] = &["default", "chrome", "edge", "firefox", "brave", "opera", "yandex"];

/// Most links opened in one go, so a typo in a script can't open thousands of tabs.
pub const MAX_OPEN: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkSet {
    pub name: String,
    pub links: Vec<Link>,
    /// One of [`BROWSERS`].
    #[serde(default = "default_browser")]
    pub browser: String,
    /// Open in a new browser window instead of adding tabs to the current one.
    #[serde(default = "yes")]
    pub new_window: bool,
    #[serde(default)]
    pub updated_at: i64,
}

fn default_browser() -> String {
    "default".into()
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    sets: Vec<LinkSet>,
    /// Deleted sets, newest first (at most 50), so a delete can be undone.
    #[serde(default)]
    trash: Vec<LinkSet>,
}

fn file() -> PathBuf {
    appdata::dir().join("links.json")
}

fn load() -> Store {
    let path = file();
    let Ok(text) = std::fs::read_to_string(&path) else { return Store::default() };
    match serde_json::from_str(&text) {
        Ok(store) => store,
        Err(_) => {
            // Never throw a damaged file away: keep it next to the new one.
            let _ = std::fs::copy(&path, path.with_extension(format!("damaged-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S"))));
            Store::default()
        }
    }
}

fn save_store(store: &Store) -> Result<(), String> {
    let text = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    appdata::write_safely(&file(), text.as_bytes())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---- addresses ----------------------------------------------------------------------------------

/// "example.com/x" -> "https://example.com/x". Only web addresses are allowed.
pub fn normalize_url(text: &str) -> Result<String, String> {
    let t = text.trim().trim_matches(|c| c == '<' || c == '>' || c == '"' || c == '\'');
    if t.is_empty() {
        return Err("empty address".into());
    }
    let lower = t.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return if t.len() > 8 && !t.contains(char::is_whitespace) { Ok(t.to_string()) } else { Err(format!("\"{t}\" is not a web address")) };
    }
    if let Some((scheme, _)) = t.split_once("://") {
        return Err(format!("only web addresses (http, https) can be opened, not {scheme}:"));
    }
    if lower.starts_with("javascript:") || lower.starts_with("file:") || lower.starts_with("data:") {
        return Err(format!("only web addresses (http, https) can be opened, not \"{t}\""));
    }
    // A bare domain: something.tld[/...]
    let host = t.split(['/', '?', '#']).next().unwrap_or("");
    let looks_like_host = host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || "-.:".contains(c) || !c.is_ascii());
    if looks_like_host && !t.contains(char::is_whitespace) {
        Ok(format!("https://{t}"))
    } else {
        Err(format!("\"{t}\" is not a web address"))
    }
}

/// Links from pasted text: one per line, optionally with a title ("Title | url", "Title - url",
/// "title,url", or just the address). Lines without an address are skipped.
pub fn parse_links(text: &str) -> Vec<Link> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '|').filter(|t| !t.is_empty()).collect();
        let found = tokens.iter().find_map(|t| {
            let lower = t.to_lowercase();
            if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
                normalize_url(t).ok().map(|u| (*t, u))
            } else {
                None
            }
        });
        let (raw, url) = match found {
            Some(hit) => hit,
            // Bare domains only when the whole line is one.
            None if tokens.len() == 1 => match normalize_url(tokens[0]) {
                Ok(u) => (tokens[0], u),
                Err(_) => continue,
            },
            None => continue,
        };
        let title = line.replacen(raw, "", 1);
        let title = title.trim().trim_matches(|c: char| c == '|' || c == '-' || c == ',' || c == ';' || c == ':' || c.is_whitespace()).to_string();
        out.push(Link { url, title });
    }
    out
}

// ---- opening --------------------------------------------------------------------------------------

fn browser_key(name: &str) -> Result<&'static str, String> {
    let key = name.trim().to_lowercase();
    let key = match key.as_str() {
        "" | "default" | "system" => "default",
        "chrome" | "google chrome" => "chrome",
        "edge" | "msedge" | "microsoft edge" => "edge",
        "firefox" | "mozilla firefox" => "firefox",
        "brave" => "brave",
        "opera" => "opera",
        "yandex" | "yandex browser" => "yandex",
        other => return Err(format!("unknown browser \"{other}\"; use one of {}", BROWSERS.join(", "))),
    };
    Ok(key)
}

#[cfg(windows)]
fn browser_program(key: &str) -> Option<PathBuf> {
    let stem = match key {
        "chrome" => "chrome",
        "edge" => "msedge",
        "firefox" => "firefox",
        "brave" => "brave",
        "opera" => "opera",
        "yandex" => "browser",
        _ => return None,
    };
    if let Some(path) = crate::lua::system::program_path(stem) {
        return Some(path);
    }
    // Usual install places, in case the program isn't in the "App Paths" list.
    let env = |k: &str| std::env::var(k).ok().map(PathBuf::from);
    let candidates: Vec<PathBuf> = match key {
        "chrome" => [env("ProgramFiles"), env("ProgramFiles(x86)"), env("LOCALAPPDATA")].into_iter().flatten().map(|b| b.join(r"Google\Chrome\Application\chrome.exe")).collect(),
        "edge" => [env("ProgramFiles(x86)"), env("ProgramFiles")].into_iter().flatten().map(|b| b.join(r"Microsoft\Edge\Application\msedge.exe")).collect(),
        "firefox" => [env("ProgramFiles"), env("ProgramFiles(x86)")].into_iter().flatten().map(|b| b.join(r"Mozilla Firefox\firefox.exe")).collect(),
        "brave" => [env("ProgramFiles"), env("LOCALAPPDATA")].into_iter().flatten().map(|b| b.join(r"BraveSoftware\Brave-Browser\Application\brave.exe")).collect(),
        "opera" => env("LOCALAPPDATA").map(|b| vec![b.join(r"Programs\Opera\opera.exe")]).unwrap_or_default(),
        "yandex" => env("LOCALAPPDATA").map(|b| vec![b.join(r"Yandex\YandexBrowser\Application\browser.exe")]).unwrap_or_default(),
        _ => Vec::new(),
    };
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "macos")]
fn mac_app(key: &str) -> Option<&'static str> {
    Some(match key {
        "chrome" => "Google Chrome",
        "edge" => "Microsoft Edge",
        "firefox" => "Firefox",
        "brave" => "Brave Browser",
        "opera" => "Opera",
        "yandex" => "Yandex",
        _ => return None,
    })
}

/// Groups of addresses small enough for one command line (Windows allows ~32,000 characters).
pub fn batches(urls: &[String], max_chars: usize) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut len = 0;
    for url in urls {
        if out.is_empty() || len + url.len() + 3 > max_chars {
            out.push(Vec::new());
            len = 0;
        }
        len += url.len() + 3;
        out.last_mut().unwrap().push(url.clone());
    }
    out
}

fn spawn(program: &std::path::Path, args: &[String]) -> Result<(), String> {
    std::process::Command::new(program).args(args).spawn().map(|_| ()).map_err(|e| format!("could not start {}: {e}", program.display()))
}

/// Open addresses in a browser. Returns how many were opened.
pub fn open_urls(urls: &[String], browser: &str, new_window: bool) -> Result<usize, String> {
    if urls.is_empty() {
        return Err("there are no links to open".into());
    }
    if urls.len() > MAX_OPEN {
        return Err(format!("{} links is more than LocalFlow opens at once ({MAX_OPEN}); split the set", urls.len()));
    }
    let key = browser_key(browser)?;
    let pause = || std::thread::sleep(Duration::from_millis(250));

    if key == "default" {
        for (i, url) in urls.iter().enumerate() {
            crate::lua::system::shell_open(url)?;
            // The first one may have to start the browser; give it a moment.
            std::thread::sleep(Duration::from_millis(if i == 0 { 1500 } else { 150 }));
        }
        return Ok(urls.len());
    }

    #[cfg(windows)]
    {
        let program = browser_program(key).ok_or_else(|| format!("{key} doesn't seem to be installed (use \"default\" for your default browser)"))?;
        if key == "firefox" {
            // Firefox takes one address per option; open them one after the other.
            for (i, url) in urls.iter().enumerate() {
                let flag = if i == 0 && new_window { "-new-window" } else { "-new-tab" };
                spawn(&program, &[flag.to_string(), url.clone()])?;
                std::thread::sleep(Duration::from_millis(if i == 0 { 1500 } else { 200 }));
            }
            return Ok(urls.len());
        }
        // Chromium browsers (Chrome, Edge, Brave, Opera, Yandex) take many addresses at once;
        // later batches land in the window the first batch opened.
        for (i, batch) in batches(urls, 20_000).into_iter().enumerate() {
            let mut args = Vec::new();
            if i == 0 && new_window {
                args.push("--new-window".to_string());
            }
            args.extend(batch);
            spawn(&program, &args)?;
            pause();
            std::thread::sleep(Duration::from_millis(if i == 0 { 1200 } else { 0 }));
        }
        Ok(urls.len())
    }
    #[cfg(target_os = "macos")]
    {
        let app = mac_app(key).ok_or("unknown browser")?;
        let mut args = vec!["-a".to_string(), app.to_string()];
        args.extend(urls.iter().cloned());
        let _ = new_window; // macOS `open` adds tabs to the browser's current window.
        spawn(std::path::Path::new("open"), &args)?;
        pause();
        Ok(urls.len())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (new_window, pause);
        Err(format!("opening in {key} is only supported on Windows and macOS; use \"default\""))
    }
}

// ---- bookmarks ------------------------------------------------------------------------------------

/// Chromium-style bookmark files of a browser, one per profile.
fn bookmark_files(key: &str) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        let local = std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
        let roaming = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_default();
        match key {
            "chrome" => roots.push(local.join(r"Google\Chrome\User Data")),
            "edge" => roots.push(local.join(r"Microsoft\Edge\User Data")),
            "brave" => roots.push(local.join(r"BraveSoftware\Brave-Browser\User Data")),
            "yandex" => roots.push(local.join(r"Yandex\YandexBrowser\User Data")),
            "opera" => return vec![roaming.join(r"Opera Software\Opera Stable\Bookmarks"), roaming.join(r"Opera Software\Opera Stable\Default\Bookmarks")],
            _ => {}
        }
    }
    #[cfg(target_os = "macos")]
    {
        let support = dirs_home().join("Library/Application Support");
        match key {
            "chrome" => roots.push(support.join("Google/Chrome")),
            "edge" => roots.push(support.join("Microsoft Edge")),
            "brave" => roots.push(support.join("BraveSoftware/Brave-Browser")),
            "yandex" => roots.push(support.join("Yandex/YandexBrowser")),
            "opera" => return vec![support.join("com.operasoftware.Opera/Bookmarks")],
            _ => {}
        }
    }
    let mut files = Vec::new();
    for root in roots {
        let mut profiles = vec![root.join("Default")];
        if let Ok(entries) = std::fs::read_dir(&root) {
            let mut more: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Profile "))).collect();
            more.sort();
            profiles.extend(more);
        }
        files.extend(profiles.into_iter().map(|p| p.join("Bookmarks")));
    }
    files.into_iter().filter(|f| f.is_file()).collect()
}

#[cfg(target_os = "macos")]
fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// The links of the first bookmarks folder called `folder` (subfolders included), from a
/// Chromium `Bookmarks` JSON file.
pub fn folder_links(json: &serde_json::Value, folder: &str) -> Option<Vec<Link>> {
    fn find<'a>(node: &'a serde_json::Value, folder: &str) -> Option<&'a serde_json::Value> {
        if node["type"] == "folder" && node["name"].as_str().is_some_and(|n| n.trim().eq_ignore_ascii_case(folder.trim())) {
            return Some(node);
        }
        node["children"].as_array()?.iter().find_map(|c| find(c, folder))
    }
    fn collect(node: &serde_json::Value, out: &mut Vec<Link>) {
        for child in node["children"].as_array().into_iter().flatten() {
            match child["type"].as_str() {
                Some("url") => {
                    if let Some(url) = child["url"].as_str().and_then(|u| normalize_url(u).ok()) {
                        out.push(Link { url, title: child["name"].as_str().unwrap_or("").to_string() });
                    }
                }
                Some("folder") => collect(child, out),
                _ => {}
            }
        }
    }
    let roots = json["roots"].as_object()?;
    let node = roots.values().find_map(|root| find(root, folder))?;
    let mut out = Vec::new();
    collect(node, &mut out);
    Some(out)
}

/// Read a bookmarks folder from a browser (Chrome, Edge, Brave, Opera, Yandex). Read-only.
/// Tip: in the browser, "Bookmark all tabs" (Ctrl+Shift+D) saves the open tabs into a folder.
pub fn import_bookmarks(folder: &str, browser: &str) -> Result<Vec<Link>, String> {
    let key = browser_key(browser)?;
    let keys: Vec<&str> = if key == "default" { vec!["chrome", "edge", "brave", "yandex", "opera"] } else { vec![key] };
    if keys == ["firefox"] {
        return Err("Firefox keeps bookmarks in a database LocalFlow doesn't read; export them as HTML or copy the addresses and paste them".into());
    }
    let mut searched = 0;
    for k in keys {
        for file in bookmark_files(k) {
            searched += 1;
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            if let Some(links) = folder_links(&json, folder) {
                return Ok(links);
            }
        }
    }
    if searched == 0 {
        Err(format!("no bookmarks found for {browser}"))
    } else {
        Err(format!("no bookmarks folder called \"{folder}\""))
    }
}

// ---- sets -------------------------------------------------------------------------------------------

pub fn list() -> Vec<LinkSet> {
    load().sets
}

pub fn trash() -> Vec<LinkSet> {
    load().trash
}

pub fn get(name: &str) -> Option<LinkSet> {
    load().sets.into_iter().find(|s| s.name.eq_ignore_ascii_case(name.trim()))
}

fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a link set needs a name".into());
    }
    if name.chars().count() > 80 {
        return Err("the name is too long (80 characters at most)".into());
    }
    Ok(name.to_string())
}

/// Create or replace a set. Addresses are checked; bad ones are an error listing them.
pub fn save(mut set: LinkSet) -> Result<LinkSet, String> {
    set.name = check_name(&set.name)?;
    set.browser = browser_key(&set.browser)?.to_string();
    let mut bad = Vec::new();
    for link in &mut set.links {
        match normalize_url(&link.url) {
            Ok(url) => link.url = url,
            Err(_) => bad.push(link.url.clone()),
        }
        link.title = link.title.trim().to_string();
    }
    if !bad.is_empty() {
        return Err(format!("not web addresses: {}", bad.join(", ")));
    }
    set.updated_at = now();
    let mut store = load();
    match store.sets.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&set.name)) {
        Some(existing) => *existing = set.clone(),
        None => store.sets.push(set.clone()),
    }
    save_store(&store)?;
    Ok(set)
}

pub fn rename(old: &str, new: &str) -> Result<(), String> {
    let new = check_name(new)?;
    let mut store = load();
    if !old.eq_ignore_ascii_case(&new) && store.sets.iter().any(|s| s.name.eq_ignore_ascii_case(&new)) {
        return Err(format!("there is already a set called \"{new}\""));
    }
    let set = store.sets.iter_mut().find(|s| s.name.eq_ignore_ascii_case(old.trim())).ok_or_else(|| format!("no link set called \"{old}\""))?;
    set.name = new;
    set.updated_at = now();
    save_store(&store)
}

/// Move a set to the trash (it can be restored).
pub fn delete(name: &str) -> Result<bool, String> {
    let mut store = load();
    let Some(i) = store.sets.iter().position(|s| s.name.eq_ignore_ascii_case(name.trim())) else { return Ok(false) };
    let set = store.sets.remove(i);
    store.trash.insert(0, set);
    store.trash.truncate(50);
    save_store(&store)?;
    Ok(true)
}

pub fn restore(name: &str) -> Result<(), String> {
    let mut store = load();
    let i = store.trash.iter().position(|s| s.name.eq_ignore_ascii_case(name.trim())).ok_or_else(|| format!("no deleted set called \"{name}\""))?;
    let mut set = store.trash.remove(i);
    while store.sets.iter().any(|s| s.name.eq_ignore_ascii_case(&set.name)) {
        set.name = format!("{} (restored)", set.name);
    }
    store.sets.push(set);
    save_store(&store)
}

/// Open a saved set with its own browser and window settings.
pub fn open_set(name: &str) -> Result<usize, String> {
    let set = get(name).ok_or_else(|| format!("no link set called \"{name}\""))?;
    let urls: Vec<String> = set.links.iter().map(|l| l.url.clone()).collect();
    open_urls(&urls, &set.browser, set.new_window)
}

// ---- Lua ----------------------------------------------------------------------------------------------

fn err(function: &str, e: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(format!("links.{function}: {e}"))
}

/// Links from a Lua value: a list of strings, a list of {url=, title=}, or pasted text.
fn links_from(value: &Value) -> Result<Vec<Link>, String> {
    match value {
        Value::String(s) => Ok(parse_links(&s.to_str().map_err(|e| e.to_string())?)),
        Value::Table(t) => {
            let mut out = Vec::new();
            for item in t.sequence_values::<Value>() {
                match item.map_err(|e| e.to_string())? {
                    Value::String(s) => out.push(Link { url: s.to_str().map_err(|e| e.to_string())?.to_string(), title: String::new() }),
                    Value::Table(row) => {
                        let url: String = row.get("url").map_err(|_| "each link needs a url".to_string())?;
                        let title: Option<String> = row.get("title").unwrap_or(None);
                        out.push(Link { url, title: title.unwrap_or_default() });
                    }
                    _ => return Err("links must be addresses (text) or { url = ..., title = ... }".into()),
                }
            }
            Ok(out)
        }
        _ => Err("give a list of addresses or text with one address per line".into()),
    }
}

fn set_table(lua: &Lua, set: &LinkSet) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("name", set.name.as_str())?;
    t.set("count", set.links.len())?;
    t.set("browser", set.browser.as_str())?;
    t.set("new_window", set.new_window)?;
    Ok(t)
}

pub fn register(lua: &Lua) -> mlua::Result<()> {
    let links = lua.create_table()?;

    links.set(
        "list",
        lua.create_function(|lua, ()| {
            let t = lua.create_table()?;
            for set in list() {
                t.push(set_table(lua, &set)?)?;
            }
            Ok(t)
        })?,
    )?;
    links.set(
        "get",
        lua.create_function(|lua, name: String| match get(&name) {
            Some(set) => Ok(Value::Table(lua.create_sequence_from(set.links.into_iter().map(|l| l.url))?)),
            None => Ok(Value::Nil),
        })?,
    )?;
    links.set(
        "items",
        lua.create_function(|lua, name: String| {
            let set = get(&name).ok_or_else(|| err("items", format!("no link set called \"{name}\"")))?;
            let t = lua.create_table()?;
            for l in set.links {
                let row = lua.create_table()?;
                row.set("url", l.url)?;
                row.set("title", l.title)?;
                t.push(row)?;
            }
            Ok(t)
        })?,
    )?;
    links.set(
        "save",
        lua.create_function(|_, (name, value, options): (String, Value, Option<Table>)| {
            let list = links_from(&value).map_err(|e| err("save", e))?;
            let existing = get(&name);
            let browser = options.as_ref().and_then(|o| o.get::<Option<String>>("browser").ok().flatten())
                .or_else(|| existing.as_ref().map(|s| s.browser.clone()))
                .unwrap_or_else(default_browser);
            let new_window = options.as_ref().and_then(|o| o.get::<Option<bool>>("new_window").ok().flatten())
                .or_else(|| existing.as_ref().map(|s| s.new_window))
                .unwrap_or(true);
            let set = save(LinkSet { name, links: list, browser, new_window, updated_at: 0 }).map_err(|e| err("save", e))?;
            Ok(set.links.len())
        })?,
    )?;
    links.set(
        "add",
        lua.create_function(|_, (name, url, title): (String, String, Option<String>)| {
            let mut set = get(&name).unwrap_or(LinkSet { name: name.clone(), links: Vec::new(), browser: default_browser(), new_window: true, updated_at: 0 });
            let url = normalize_url(&url).map_err(|e| err("add", e))?;
            if !set.links.iter().any(|l| l.url == url) {
                set.links.push(Link { url, title: title.unwrap_or_default() });
            }
            let set = save(set).map_err(|e| err("add", e))?;
            Ok(set.links.len())
        })?,
    )?;
    links.set(
        "remove",
        lua.create_function(|_, (name, what): (String, Value)| {
            let mut set = get(&name).ok_or_else(|| err("remove", format!("no link set called \"{name}\"")))?;
            let before = set.links.len();
            match what {
                Value::Integer(i) if i >= 1 && (i as usize) <= set.links.len() => {
                    set.links.remove(i as usize - 1);
                }
                Value::String(s) => {
                    let url = normalize_url(&s.to_str()?).unwrap_or_else(|_| s.to_str().map(|s| s.to_string()).unwrap_or_default());
                    set.links.retain(|l| l.url != url);
                }
                _ => return Err(err("remove", "give the address or its number in the list")),
            }
            let removed = before - set.links.len();
            save(set).map_err(|e| err("remove", e))?;
            Ok(removed)
        })?,
    )?;
    links.set(
        "delete",
        lua.create_function(|_, name: String| delete(&name).map_err(|e| err("delete", e)))?,
    )?;
    links.set(
        "open",
        lua.create_function(|_, (what, options): (Value, Option<Table>)| {
            let browser: Option<String> = options.as_ref().and_then(|o| o.get("browser").ok().flatten());
            let new_window: Option<bool> = options.as_ref().and_then(|o| o.get("new_window").ok().flatten());
            let (urls, set_browser, set_window) = match &what {
                Value::String(s) if !s.to_str()?.contains("://") && get(&s.to_str()?).is_some() => {
                    let set = get(&s.to_str()?).unwrap();
                    (set.links.iter().map(|l| l.url.clone()).collect::<Vec<_>>(), set.browser, set.new_window)
                }
                Value::String(s) if !s.to_str()?.contains("://") && !s.to_str()?.contains('\n') && !s.to_str()?.contains('.') => {
                    return Err(err("open", format!("no link set called \"{}\"", s.to_str()?)));
                }
                other => {
                    let list = links_from(other).map_err(|e| err("open", e))?;
                    let urls = list.iter().map(|l| normalize_url(&l.url)).collect::<Result<Vec<_>, _>>().map_err(|e| err("open", e))?;
                    (urls, default_browser(), true)
                }
            };
            open_urls(&urls, browser.as_deref().unwrap_or(&set_browser), new_window.unwrap_or(set_window)).map_err(|e| err("open", e))
        })?,
    )?;
    links.set(
        "import_bookmarks",
        lua.create_function(|lua, (folder, browser, save_as): (String, Option<String>, Option<String>)| {
            let list = import_bookmarks(&folder, browser.as_deref().unwrap_or("default")).map_err(|e| err("import_bookmarks", e))?;
            if let Some(name) = save_as {
                let browser = browser.clone().unwrap_or_else(default_browser);
                save(LinkSet { name, links: list.clone(), browser, new_window: true, updated_at: 0 }).map_err(|e| err("import_bookmarks", e))?;
            }
            lua.create_sequence_from(list.into_iter().map(|l| l.url))
        })?,
    )?;
    links.set(
        "parse",
        lua.create_function(|lua, text: String| lua.create_sequence_from(parse_links(&text).into_iter().map(|l| l.url)))?,
    )?;

    lua.globals().set("links", links)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_normalized_and_checked() {
        assert_eq!(normalize_url("example.com/a?b=1").unwrap(), "https://example.com/a?b=1");
        assert_eq!(normalize_url(" https://x.org ").unwrap(), "https://x.org");
        assert_eq!(normalize_url("www.site.ru").unwrap(), "https://www.site.ru");
        assert!(normalize_url("javascript:alert(1)").is_err());
        assert!(normalize_url("file:///C:/x").is_err());
        assert!(normalize_url("ftp://x.org").is_err());
        assert!(normalize_url("not an address").is_err());
        assert!(normalize_url("hello").is_err());
    }

    #[test]
    fn pasted_text_becomes_links() {
        let text = "# work\nMail | https://mail.example.com\nhttps://docs.example.com/page\nTracker - tracker.example.com\nnotes,https://n.example.com\nnothing here\n\nexample.org";
        let links = parse_links(text);
        let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, ["https://mail.example.com", "https://docs.example.com/page", "https://n.example.com", "https://example.org"]);
        assert_eq!(links[0].title, "Mail");
        // "Tracker - tracker.example.com" has no http/www token and more than one word: skipped.
        let links = parse_links("notes,https://n.example.com");
        assert_eq!(links[0].title, "notes");
    }

    #[test]
    fn long_lists_are_split_for_the_command_line() {
        let urls: Vec<String> = (0..50).map(|i| format!("https://example.com/{i:0>400}")).collect();
        let parts = batches(&urls, 20_000);
        assert!(parts.len() >= 2);
        assert_eq!(parts.iter().map(|p| p.len()).sum::<usize>(), 50);
        assert!(parts.iter().all(|p| p.iter().map(|u| u.len() + 3).sum::<usize>() <= 20_000));
    }

    #[test]
    fn bookmark_folders_are_found_with_subfolders() {
        let json = serde_json::json!({ "roots": {
            "bookmark_bar": { "type": "folder", "name": "Bookmarks bar", "children": [
                { "type": "folder", "name": "Work tabs", "children": [
                    { "type": "url", "name": "Mail", "url": "https://mail.example.com" },
                    { "type": "folder", "name": "Docs", "children": [ { "type": "url", "name": "D", "url": "https://docs.example.com" } ] },
                    { "type": "url", "name": "Bad", "url": "javascript:void(0)" }
                ]}
            ]},
            "other": { "type": "folder", "name": "Other", "children": [] }
        }});
        let links = folder_links(&json, "work TABS").unwrap();
        assert_eq!(links.iter().map(|l| l.url.as_str()).collect::<Vec<_>>(), ["https://mail.example.com", "https://docs.example.com"]);
        assert!(folder_links(&json, "Missing").is_none());
    }

    #[test]
    fn sets_are_saved_trashed_and_restored() {
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("LOCALFLOW_DATA_DIR", dir.path());
        let set = LinkSet { name: "Work".into(), links: parse_links("a.example.com\nhttps://b.example.com"), browser: "Chrome".into(), new_window: true, updated_at: 0 };
        save(set).unwrap();
        assert_eq!(get("work").unwrap().browser, "chrome");
        assert_eq!(get("Work").unwrap().links.len(), 2);
        assert!(save(LinkSet { name: "Bad".into(), links: vec![Link { url: "javascript:x".into(), title: String::new() }], browser: "default".into(), new_window: true, updated_at: 0 }).is_err());
        // A second save keeps the first version as a backup.
        save(LinkSet { name: "Work".into(), links: parse_links("c.example.com"), browser: "edge".into(), new_window: false, updated_at: 0 }).unwrap();
        assert!(dir.path().join("links.bak").exists());
        assert!(delete("WORK").unwrap());
        assert!(get("Work").is_none());
        assert_eq!(trash().len(), 1);
        restore("Work").unwrap();
        assert_eq!(get("Work").unwrap().links[0].url, "https://c.example.com");
        // A damaged file is kept, not thrown away.
        std::fs::write(dir.path().join("links.json"), "{ not json").unwrap();
        assert!(list().is_empty());
        assert!(std::fs::read_dir(dir.path()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("damaged")));
        assert!(open_urls(&[], "default", true).is_err());
        assert!(open_urls(&vec!["https://x.org".into(); MAX_OPEN + 1], "default", true).unwrap_err().contains("more than"));
        assert!(browser_key("netscape").is_err());
    }
}
