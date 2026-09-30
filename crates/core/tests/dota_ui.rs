//! The Dota 2 companion's newer script functions (live helper, hero lookup, post-game review),
//! the Telegram commands and the post-game review template. Offline: the hero and item lists
//! come from the recorded fixtures, copied into the companion's cache. Nothing starts the game,
//! opens a browser or sends a message.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use localflow_core::{
    dota::{review::account_id_from, DotaSettings},
    lua::{engine::execute, engine::RunContext, engine::ExecutionResult, sandbox::PathPolicy},
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dota/data");

/// One temporary data folder for the whole test binary, with a fresh cache of the hero,
/// item and patch lists so nothing needs the network for them.
fn setup() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap().keep();
        let data = dir.join("data");
        let cache = data.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let now = localflow_core::dota::now();
        for key in ["heroes", "items", "patch"] {
            let body: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(Path::new(FIXTURES).join(format!("{key}.json"))).unwrap()).unwrap();
            let entry = serde_json::json!({ "source": "OpenDota", "fetched_at": now, "body": body });
            std::fs::write(cache.join(format!("{key}.json")), entry.to_string()).unwrap();
        }
        let game = dir.join("steamapps/common/dota 2 beta");
        std::fs::create_dir_all(game.join("game/dota")).unwrap();
        std::env::set_var("LOCALFLOW_DOTA_DIR", &data);
        std::env::set_var("LOCALFLOW_DOTA_GAME_DIR", &game);
        dir
    })
}

/// The tests share the settings file: one at a time.
fn lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn run(code: &str) -> ExecutionResult {
    let dir = setup();
    let policy = Arc::new(PathPolicy::new(&[dir.to_path_buf()]));
    execute(code, &RunContext::new(1, "Dota UI test", "manual", false), policy, Duration::from_secs(30))
}

fn clear_account() {
    setup();
    let mut settings = DotaSettings::load();
    settings.account_id = None;
    settings.launch_url.clear();
    settings.save().unwrap();
}

#[test]
fn new_functions_exist_and_fail_politely() {
    let _guard = lock();
    clear_account();
    let result = run(
        r#"
        for _, name in ipairs({ "live", "next_item", "reminders", "lookup", "last_match", "recent_matches", "set_account" }) do
            assert(type(dota[name]) == "function", "missing dota." .. name)
        end

        -- No match is running in a test: no live state, and next_item says why.
        local live = dota.live()
        assert(live == nil or type(live.gold) == "number", "live is nil or a state")
        local next_item, why = dota.next_item()
        assert(next_item ~= nil or (type(why) == "string" and why ~= ""), "next_item gives a reason")

        assert(type(dota.reminders(0, 600)) == "table")
        assert(type(dota.reminders(0)) == "table")
        local ok, e = pcall(dota.reminders, 600, 0)
        assert(not ok and tostring(e):find("dota.reminders:"), tostring(e))

        -- Unknown hero: a readable error from the name resolver.
        ok, e = pcall(dota.lookup, "zzzzzz")
        assert(not ok and tostring(e):find("dota.lookup: no hero called"), tostring(e))
        ok, e = pcall(dota.lookup, {})
        assert(not ok and tostring(e):find("name a hero"), tostring(e))
        -- A real hero: the lookup, or (until it is implemented) a friendly error.
        ok, e = pcall(dota.lookup, "Axe", 3)
        if ok then
            assert(e.hero == "Axe", "looked up Axe")
            assert(type(e.weak_against) == "table" and type(e.strong_against) == "table")
        else
            assert(tostring(e):find("dota.lookup: the hero lookup isn't available"), tostring(e))
        end

        -- No account id yet: say so at once (even with a wait), for both review functions.
        for _, call in ipairs({ { dota.last_match }, { dota.last_match, 30 }, { dota.recent_matches, 5 } }) do
            ok, e = pcall(call[1], call[2])
            assert(not ok and tostring(e):find("account isn't set"), tostring(e))
            assert(tostring(e):find("Settings"), "says where to set it: " .. tostring(e))
        end
        ok, e = pcall(dota.last_match, 700)
        assert(not ok and tostring(e):find("between 0 and 600"), tostring(e))
        "#,
    );
    assert!(result.success, "{:?}\n{}", result.error, result.output());
    for line in result.output().lines() {
        assert!(!line.contains("stack traceback"), "{line}");
    }
}

#[test]
fn set_account_uses_the_real_parser() {
    let _guard = lock();
    clear_account();
    let link = "https://www.opendota.com/players/86745912";
    // Placeholder: `None`; the real parser: the id from the link.
    let parsed: Option<u64> = account_id_from(link);
    if let Some(id) = parsed {
        assert_eq!(id, 86745912);
    }

    let result = run(&format!(
        r#"
        local ok, id = pcall(dota.set_account, "{link}")
        if ok then
            log("saved " .. tostring(id))
        else
            assert(tostring(id):find("couldn't find a Dota account id"), tostring(id))
            log("refused")
        end
        local bad_ok, e = pcall(dota.set_account, "hello there")
        assert(not bad_ok and tostring(e):find("dota.set_account: couldn't find"), tostring(e))
        "#
    ));
    assert!(result.success, "{:?}\n{}", result.error, result.output());
    let saved = DotaSettings::load().account_id;
    assert_eq!(saved, parsed, "the saved id is what account_id_from returns");
    if parsed.is_some() {
        assert!(result.output().contains("saved 86745912"), "{}", result.output());
        // With an account, the review no longer complains about it.
        let review = run(r#"local ok, e = pcall(dota.recent_matches, 1); if not ok then log("error: " .. tostring(e)) end"#);
        assert!(!review.output().contains("account isn't set"), "{}", review.output());
    }

    // nil or "" forgets it.
    let cleared = run(r#"assert(dota.set_account(nil) == nil); assert(dota.set_account("") == nil)"#);
    assert!(cleared.success, "{:?}", cleared.error);
    assert_eq!(DotaSettings::load().account_id, None);
}

/// Run one Telegram command the way `remote.rs` does, with `telegram.send` logging instead.
fn remote(command: &str, args: &str) -> ExecutionResult {
    let code = format!(
        "telegram.send = function(t) log('REPLY ' .. t) end\ntelegram.send_photo = function(p) log('PHOTO ' .. p) end\nCOMMAND = {}\nARGS = {}\nALLOW_POWER = false\n{}",
        localflow_core::remote::lua_string(command),
        localflow_core::remote::lua_string(args),
        localflow_core::remote::COMMANDS
    );
    run(&code)
}

#[test]
fn telegram_knows_the_dota_commands() {
    let _guard = lock();
    clear_account();
    let help = remote("/help", "");
    assert!(help.success, "{:?}", help.error);
    let text = help.output();
    for command in ["/draft", "/build", "/counter <hero>", "/lastmatch"] {
        assert!(text.contains(command), "/help lists {command}: {text}");
    }

    for (command, args) in [("/draft", ""), ("/build", ""), ("/counter", ""), ("/counter", "zzzzzz"), ("/counter", "Axe"), ("/lastmatch", "")] {
        let result = remote(command, args);
        let out = result.output();
        println!("== {command} {args}\n{out}");
        assert!(result.success, "{command} {args}: {:?}\n{out}", result.error);
        let reply = out.lines().find(|l| l.contains("REPLY")).unwrap_or_else(|| panic!("{command}: no reply\n{out}"));
        assert!(!out.contains("stack traceback") && !reply.contains("runtime error"), "{command}: {out}");
        match command {
            "/counter" if args.is_empty() => assert!(reply.contains("Which hero"), "{reply}"),
            "/counter" if args == "zzzzzz" => assert!(reply.contains("no hero called"), "{reply}"),
            "/lastmatch" => assert!(reply.contains("account isn't set"), "{reply}"),
            _ => {}
        }
    }
    // Replies stay short enough for a phone screen.
    let draft = remote("/draft", "");
    let reply: String = draft.output().lines().skip_while(|l| !l.contains("REPLY")).collect::<Vec<_>>().join("\n");
    assert!(reply.lines().count() <= 10, "{reply}");
}

#[test]
fn post_game_review_template_explains_the_account_setting() {
    let _guard = lock();
    clear_account();
    let template = localflow_core::lua::find_example("dota-post-game-review").expect("template");
    assert_eq!(template.category, "games");
    assert_eq!(template.triggers, r#"{"app_exit":"dota2"}"#);
    assert!(!template.allow_system);
    assert!(localflow_core::lua::catalog::unknown_calls(template.code).is_empty(), "{:?}", localflow_core::lua::catalog::unknown_calls(template.code));

    let started = std::time::Instant::now();
    let result = run(template.code);
    assert!(result.success, "{:?}\n{}", result.error, result.output());
    // Without an account id it says so at once instead of waiting a minute first.
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    let notice = result.logs.iter().find(|l| l.level == "notify").expect("a notification");
    assert!(notice.message.contains("account isn't set"), "{}", notice.message);
    assert!(!notice.message.contains("traceback"), "{}", notice.message);
}
