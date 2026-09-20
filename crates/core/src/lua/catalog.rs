//! The exact list of functions scripts can call, taken from a real sandbox, and a
//! checker that finds calls to functions that don't exist (e.g. `close()` or
//! `fs.remove()`). Used to tell the AI what's available and to check its code.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use mlua::{Table, Value};

use super::{api, data::StoreState, lualib, sandbox};

/// Everything a script can call.
#[derive(Debug, Default)]
pub struct Catalog {
    /// Top-level functions, e.g. `log`, `notify`, `ipairs`.
    pub functions: BTreeSet<String>,
    /// Functions inside tables, by table: `fs` → { `list`, `move`, … }.
    pub tables: BTreeMap<String, BTreeSet<String>>,
    /// The helper modules: `lf.strings` → { `trim`, … }.
    pub modules: BTreeMap<String, BTreeSet<String>>,
}

impl Catalog {
    pub fn has(&self, table: &str, function: &str) -> bool {
        self.tables.get(table).is_some_and(|f| f.contains(function))
    }

    /// Every function as "fs.list", "log", …
    pub fn all(&self) -> Vec<String> {
        let mut all: Vec<String> = self.functions.iter().cloned().collect();
        for (table, functions) in &self.tables {
            all.extend(functions.iter().map(|f| format!("{table}.{f}")));
        }
        all
    }

    /// One line per table, for the AI: "fs: append, basename, copy, …".
    pub fn summary(&self) -> String {
        let mut lines = vec![format!("functions: {}", self.functions.iter().cloned().collect::<Vec<_>>().join(", "))];
        for (table, functions) in &self.tables {
            lines.push(format!("{table}: {}", functions.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        for (module, functions) in &self.modules {
            lines.push(format!("require(\"{module}\"): {}", functions.iter().cloned().collect::<Vec<_>>().join(", ")));
        }
        lines.join("\n")
    }
}

fn function_names(table: &Table) -> BTreeSet<String> {
    table
        .pairs::<Value, Value>()
        .flatten()
        .filter_map(|(k, v)| match (k, v) {
            (Value::String(k), Value::Function(_)) => Some(k.to_string_lossy()),
            _ => None,
        })
        .collect()
}

fn build() -> mlua::Result<Catalog> {
    let lua = sandbox::new_lua(Duration::from_secs(5))?;
    let logs = Rc::new(api::LogCollector::new(|_| {}));
    let store = Rc::new(RefCell::new(StoreState::default()));
    let policy = Arc::new(sandbox::PathPolicy::new(&[]));
    // With system control on, so every function is listed.
    api::register(&lua, policy, logs, Instant::now() + Duration::from_secs(5), store, true)?;

    let mut catalog = Catalog::default();
    for pair in lua.globals().pairs::<Value, Value>() {
        let (Value::String(name), value) = pair? else { continue };
        let name = name.to_string_lossy();
        match value {
            Value::Function(_) => {
                catalog.functions.insert(name);
            }
            Value::Table(table) if name != "_G" => {
                let functions = function_names(&table);
                if !functions.is_empty() {
                    catalog.tables.insert(name, functions);
                }
            }
            _ => {}
        }
    }
    // Registered per run by the engine, not by api::register.
    catalog.tables.insert("automations".into(), ["call", "list", "run"].map(String::from).into());

    for (module, _) in lualib::MODULES {
        let loaded: Value = lua.load(format!("return require(\"{module}\")")).call(())?;
        if let Value::Table(table) = loaded {
            catalog.modules.insert(module.to_string(), function_names(&table));
        }
    }
    Ok(catalog)
}

/// The catalog, built once.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| build().unwrap_or_default())
}

// ---- checking code ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    /// A string literal (its contents don't matter here).
    Text,
    Symbol(char),
    Other,
}

/// Split Lua code into names, strings and symbols, skipping comments.
fn tokens(code: &str) -> Vec<Token> {
    let chars: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    // `[[ ... ]]` or `[==[ ... ]==]`: returns the index after the closing bracket.
    let long_bracket_end = |start: usize| -> Option<usize> {
        let mut j = start + 1;
        let mut level = 0;
        while chars.get(j) == Some(&'=') {
            level += 1;
            j += 1;
        }
        if chars.get(j) != Some(&'[') {
            return None;
        }
        let close: String = std::iter::once(']').chain(std::iter::repeat('=').take(level)).chain(std::iter::once(']')).collect();
        let rest: String = chars[j + 1..].iter().collect();
        Some(match rest.find(&close) {
            Some(pos) => j + 1 + rest[..pos].chars().count() + close.chars().count(),
            None => chars.len(),
        })
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            if chars.get(i + 2) == Some(&'[') {
                if let Some(end) = long_bracket_end(i + 2) {
                    i = end;
                    continue;
                }
            }
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '"' || c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != c && chars[i] != '\n' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            out.push(Token::Text);
        } else if c == '[' && matches!(chars.get(i + 1), Some('[') | Some('=')) {
            match long_bracket_end(i) {
                Some(end) => {
                    i = end;
                    out.push(Token::Text);
                }
                None => {
                    out.push(Token::Symbol('['));
                    i += 1;
                }
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                i += 1;
            }
            out.push(Token::Other);
        } else if c.is_whitespace() {
            i += 1;
        } else {
            out.push(Token::Symbol(c));
            i += 1;
        }
    }
    out
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil", "not",
    "or", "repeat", "return", "then", "true", "until", "while",
];

fn is_call_start(token: Option<&Token>) -> bool {
    matches!(token, Some(Token::Symbol('(')) | Some(Token::Symbol('{')) | Some(Token::Text))
}

/// Levenshtein distance, for "did you mean".
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != *cb)).min(row[j] + 1).min(row[j + 1] + 1);
            previous = current;
        }
    }
    row[b.len()]
}

/// Up to three real functions with a similar name.
fn suggestions(candidates: Vec<String>, wanted: &str) -> Vec<String> {
    let last = wanted.rsplit('.').next().unwrap_or(wanted).to_lowercase();
    // Allow about one typo per three letters.
    let allowed = (last.chars().count() / 3).max(1);
    let mut scored: Vec<(usize, String)> = candidates
        .into_iter()
        // Lua's own coroutine/debug-style internals are never what a script meant.
        .filter(|name| !name.starts_with("coroutine."))
        .map(|name| {
            let own = name.rsplit('.').next().unwrap_or(&name).to_lowercase();
            // Same final name in another table (close → window.close) counts as very close.
            let score = if own == last { 0 } else { distance(&own, &last) + 1 };
            (score, name)
        })
        .filter(|(score, _)| *score <= allowed + 1)
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, n)| n).collect()
}

fn problem(candidates: Vec<String>, call: &str) -> String {
    let ideas = suggestions(candidates, call);
    if ideas.is_empty() {
        format!("{call}() doesn't exist in LocalFlow")
    } else {
        format!("{call}() doesn't exist in LocalFlow; did you mean {}?", ideas.join(", "))
    }
}

/// Calls to functions that don't exist, as readable problems (empty when all is well).
/// It's a quick check, not a full Lua parser: it knows about local variables,
/// function parameters, loop variables and `require("lf.…")`.
pub fn unknown_calls(code: &str) -> Vec<String> {
    let catalog = catalog();
    let tokens = tokens(code);
    let mut locals: BTreeSet<String> = BTreeSet::new();
    // local strings = require("lf.strings")
    let mut modules: BTreeMap<String, String> = BTreeMap::new();

    // First pass: names the script defines itself.
    for (i, token) in tokens.iter().enumerate() {
        let Token::Name(word) = token else { continue };
        match word.as_str() {
            "local" | "for" => {
                let mut j = i + 1;
                if tokens.get(j) == Some(&Token::Name("function".into())) {
                    j += 1;
                }
                while let Some(Token::Name(name)) = tokens.get(j) {
                    if KEYWORDS.contains(&name.as_str()) {
                        break;
                    }
                    locals.insert(name.clone());
                    j += 1;
                    // `local a <const>` / `local a, b`
                    match tokens.get(j) {
                        Some(Token::Symbol(',')) => j += 1,
                        Some(Token::Symbol('<')) => j += 3,
                        _ => break,
                    }
                }
                // local x = require("lf.module")
                if word == "local" {
                    if let (Some(Token::Name(name)), Some(Token::Symbol('=')), Some(Token::Name(req))) =
                        (tokens.get(i + 1), tokens.get(i + 2), tokens.get(i + 3))
                    {
                        if req == "require" {
                            let module = code_module_name(code, name);
                            if let Some(module) = module {
                                modules.insert(name.clone(), module);
                            }
                        }
                    }
                }
            }
            "function" => {
                let mut j = i + 1;
                // function name(...) / function t.name(...) / function t:name(...)
                if let Some(Token::Name(name)) = tokens.get(j) {
                    locals.insert(name.clone());
                    j += 1;
                    while matches!(tokens.get(j), Some(Token::Symbol('.')) | Some(Token::Symbol(':'))) {
                        j += 2;
                    }
                }
                if tokens.get(j) == Some(&Token::Symbol('(')) {
                    j += 1;
                    while let Some(t) = tokens.get(j) {
                        match t {
                            Token::Name(p) => {
                                locals.insert(p.clone());
                            }
                            Token::Symbol(')') => break,
                            _ => {}
                        }
                        j += 1;
                    }
                }
            }
            _ => {}
        }
    }
    // Table fields assigned by the script itself: `helpers.sum = function` or `function helpers.sum`.
    let mut own_fields: BTreeSet<(String, String)> = BTreeSet::new();
    for w in tokens.windows(4) {
        if let [Token::Name(t), Token::Symbol('.'), Token::Name(f), Token::Symbol('=')] = w {
            own_fields.insert((t.clone(), f.clone()));
        }
    }

    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    let mut report = |call: String, candidates: Vec<String>, problems: &mut Vec<String>| {
        if seen.insert(call.clone()) {
            problems.push(problem(candidates, &call));
        }
    };
    for i in 0..tokens.len() {
        let Token::Name(first) = &tokens[i] else { continue };
        // Only the start of an expression: not `x.first` or `x:first`.
        if matches!(i.checked_sub(1).and_then(|p| tokens.get(p)), Some(Token::Symbol('.')) | Some(Token::Symbol(':'))) {
            continue;
        }
        if KEYWORDS.contains(&first.as_str()) || i.checked_sub(1).and_then(|p| tokens.get(p)) == Some(&Token::Name("function".into())) {
            continue;
        }
        // table.something.function(...): LocalFlow has no tables inside its tables.
        if let (Some(Token::Symbol('.')), Some(Token::Name(second)), Some(Token::Symbol('.')), Some(Token::Name(third))) =
            (tokens.get(i + 1), tokens.get(i + 2), tokens.get(i + 3), tokens.get(i + 4))
        {
            if catalog.tables.contains_key(first.as_str()) && !locals.contains(first) && is_call_start(tokens.get(i + 5)) {
                report(format!("{first}.{second}.{third}"), catalog.all(), &mut problems);
                continue;
            }
        }
        match (tokens.get(i + 1), tokens.get(i + 2)) {
            // table.function(...)
            (Some(Token::Symbol('.')), Some(Token::Name(second))) if is_call_start(tokens.get(i + 3)) => {
                if own_fields.contains(&(first.clone(), second.clone())) {
                    continue;
                }
                if let Some(module) = modules.get(first) {
                    let functions = catalog.modules.get(module).cloned().unwrap_or_default();
                    if !functions.contains(second) {
                        let candidates = functions.iter().map(|f| format!("{first}.{f}")).collect();
                        report(format!("{first}.{second}"), candidates, &mut problems);
                    }
                } else if locals.contains(first) {
                    // A local table we can't see into.
                } else if catalog.tables.contains_key(first.as_str()) {
                    if !catalog.has(first, second) {
                        report(format!("{first}.{second}"), catalog.all(), &mut problems);
                    }
                } else {
                    report(format!("{first}.{second}"), catalog.all(), &mut problems);
                }
            }
            // function(...)
            (next, _) if is_call_start(next) => {
                if !locals.contains(first) && !catalog.functions.contains(first.as_str()) {
                    report(first.clone(), catalog.all(), &mut problems);
                }
            }
            _ => {}
        }
    }
    problems
}

/// The module name in `local NAME = require("lf.xxx")`, read from the source text.
fn code_module_name(code: &str, name: &str) -> Option<String> {
    code.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("local")?.trim_start();
        let rest = rest.strip_prefix(name)?.trim_start().strip_prefix('=')?.trim_start();
        let rest = rest.strip_prefix("require")?.trim_start().trim_start_matches('(').trim_start();
        let quote = rest.chars().next().filter(|q| *q == '"' || *q == '\'')?;
        let inner = &rest[1..];
        Some(inner[..inner.find(quote)?].to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_the_real_functions() {
        let c = catalog();
        for f in ["log", "notify", "wait", "ask", "print", "ipairs", "require"] {
            assert!(c.functions.contains(f), "missing {f}");
        }
        assert!(c.has("fs", "list") && c.has("window", "close") && c.has("ai", "ask") && c.has("string", "format"));
        assert!(!c.functions.contains("close") && !c.has("fs", "remove"));
        assert!(c.modules["lf.strings"].contains("trim"));
        assert!(c.summary().contains("fs: "));
    }

    #[test]
    fn invented_functions_are_found() {
        let code = r#"
            -- close() in a comment is fine
            local s = "close() in a string is fine"
            automation { name = "x", run = function(ctx)
                local files = fs.list("~/Downloads", "*.pdf")
                for _, f in ipairs(files) do
                    fs.remove(f)
                end
                close()
                system.window.close("Telegram")
                notify("done " .. #files)
            end }
        "#;
        let problems = unknown_calls(code);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].starts_with("fs.remove()") && problems[0].contains("fs."), "{problems:?}");
        assert!(problems[1].starts_with("close()") && problems[1].contains("window.close"), "{problems:?}");
        assert!(problems[2].starts_with("system.window.close()") && problems[2].contains("window.close"), "{problems:?}");
    }

    #[test]
    fn the_scripts_own_names_are_fine() {
        let code = r#"
            local strings = require("lf.strings")
            local helpers = {}
            function helpers.clean(x) return strings.trim(x) end
            local function shout(text) return text:upper() end
            local t = { add = function(a, b) return a + b end }
            for i, v in ipairs({ 1 }) do log(shout(helpers.clean(" hi ")) .. t.add(i, v)) end
            log(string.format("%d", 5), math.floor(1.5), table.concat({}, ","))
            log(strings.nope("x"))
        "#;
        let problems = unknown_calls(code);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("strings.nope()"), "{problems:?}");
        assert!(!problems[0].contains("app."), "suggest only from lf.strings: {problems:?}");
        let typo = unknown_calls(r#"local strings = require("lf.strings") log(strings.trimm(" x "))"#);
        assert!(typo[0].contains("did you mean strings.trim"), "{typo:?}");
    }

    #[test]
    fn every_template_passes_the_check() {
        for example in super::super::EXAMPLES {
            assert_eq!(unknown_calls(example.code), Vec::<String>::new(), "template {}", example.slug);
        }
    }
}
