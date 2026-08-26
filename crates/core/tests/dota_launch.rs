//! Dota 2 companion, stage 1: the Game State Integration file, the local listener, the
//! once-per-launch page, and the `dota` table in scripts. Nothing here starts the game,
//! touches the real game folder or opens a browser.

use std::{
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

use localflow_core::{
    dota::{
        launch::{self, DotaProcess, GameState, GsiListener, OpenOutcome, Phase},
        DotaSettings, DraftState, PickSource, Role, Slot, Team,
    },
    lua::{engine::execute, engine::RunContext, sandbox::PathPolicy},
};

/// One temporary folder for the whole test binary: the companion's data folder and a fake
/// game folder. Set before any test touches them.
fn setup() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap().keep();
        let game = dir.join("steamapps/common/dota 2 beta");
        std::fs::create_dir_all(game.join("game/dota")).unwrap();
        std::env::set_var("LOCALFLOW_DOTA_DIR", dir.join("data"));
        std::env::set_var("LOCALFLOW_DOTA_GAME_DIR", &game);
        dir
    })
}

fn game_dir() -> PathBuf {
    setup().join("steamapps/common/dota 2 beta")
}

// ---- the .cfg file ----------------------------------------------------------------------------

#[test]
fn config_text_has_the_address_token_and_sections() {
    let text = launch::gsi_config_text_with(4000, "abc123");
    assert!(text.starts_with("\"LocalFlow Dota 2 companion\""), "{text}");
    assert!(text.contains(r#""uri"           "http://127.0.0.1:4000/""#), "{text}");
    for section in ["provider", "map", "player", "hero"] {
        assert!(text.contains(&format!("\"{section}\"")), "missing {section}: {text}");
    }
    assert!(text.contains(r#""token"     "abc123""#), "{text}");
    // Only what the companion needs: no items, abilities or other players' data.
    for extra in ["items", "abilities", "allplayers", "draft", "wearables"] {
        assert!(!text.contains(&format!("\"{extra}\"")), "{extra} should not be requested");
    }
    assert_eq!(text.matches('{').count(), text.matches('}').count());
}

#[test]
fn the_token_is_created_once_and_kept() {
    setup();
    let first = launch::token();
    assert!(first.len() >= 32 && first.chars().all(|c| c.is_ascii_hexdigit()), "{first}");
    assert_eq!(launch::token(), first);
    assert!(launch::gsi_config_text(3417).contains(&first));
}

#[test]
fn steam_library_folders_are_read() {
    let vdf = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"0"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"apps"
		{
			"570"		"12345"
		}
	}
}"#;
    let folders = launch::parse_library_folders(vdf);
    assert_eq!(folders, vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]);
}

#[test]
fn install_and_remove_touch_only_our_file() {
    setup();
    let game = game_dir();
    let other = launch::cfg_dir(&game).join("gamestate_integration_other_app.cfg");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&other, "someone else's").unwrap();

    assert_eq!(launch::find_dota_dir(), Some(game.clone()));
    let path = launch::install_gsi(3417).unwrap();
    assert_eq!(path, game.join("game/dota/cfg/gamestate_integration/gamestate_integration_localflow.cfg"));
    assert!(launch::gsi_installed());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("http://127.0.0.1:3417/") && text.contains(&launch::token()));

    assert!(launch::uninstall_gsi().unwrap());
    assert!(!path.exists());
    assert!(!launch::gsi_installed());
    assert!(!launch::uninstall_gsi().unwrap(), "removing twice is fine");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "someone else's", "other programs' files stay");
}

#[test]
fn install_explains_where_to_put_the_file_when_it_cannot_write() {
    setup();
    let fake = tempfile::tempdir().unwrap();
    // A file where the folder should be: the folder can't be created.
    std::fs::create_dir_all(fake.path().join("game/dota")).unwrap();
    std::fs::write(fake.path().join("game/dota/cfg"), "not a folder").unwrap();
    let error = launch::install_gsi_into(fake.path(), 3417).unwrap_err();
    assert!(error.contains("gamestate_integration_localflow.cfg"), "{error}");
    assert!(error.contains("paste the text"), "{error}");
    assert!(launch::install_gsi_into(fake.path(), 0).is_err(), "port 0 is refused");
}

// ---- the listener ---------------------------------------------------------------------------------

fn post(port: u16, body: &str) -> String {
    request(port, &format!("POST / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()))
}

fn request(port: u16, raw: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer
}

const MENU: &str = r#"{
  "provider": { "name": "Dota 2", "appid": 570, "version": 47, "timestamp": 1700000000 },
  "player": { "steamid": "0", "accountid": "0", "name": "player", "activity": "menu" },
  "auth": { "token": "secret-token" }
}"#;

const HERO_SELECTION: &str = r#"{
  "provider": { "name": "Dota 2", "appid": 570, "version": 47, "timestamp": 1700000100 },
  "map": { "name": "start", "matchid": "7000000001", "game_time": 0, "clock_time": -60,
           "daytime": true, "game_state": "DOTA_GAMERULES_STATE_HERO_SELECTION", "paused": false },
  "player": { "steamid": "0", "name": "player", "activity": "playing", "team_name": "dire" },
  "hero": { "id": 2, "name": "npc_dota_hero_axe" },
  "auth": { "token": "secret-token" }
}"#;

#[test]
fn the_listener_follows_the_game_and_refuses_strangers() {
    let updates = Arc::new(Mutex::new(Vec::<(Phase, Phase)>::new()));
    let seen = updates.clone();
    let listener = GsiListener::start(
        0,
        "secret-token".into(),
        Some(Arc::new(move |before: &GameState, after: &GameState| seen.lock().unwrap().push((before.phase, after.phase)))),
    )
    .unwrap();
    let port = listener.port();
    assert_eq!(listener.state(), GameState::default());

    // The main menu: no map section.
    assert!(post(port, MENU).starts_with("HTTP/1.1 200"));
    let state = listener.state();
    assert_eq!(state.phase, Phase::Menu);
    assert_eq!((state.team, state.hero_id), (None, None));
    assert!(state.last_update.is_some());

    // Hero selection with the player's team and hero.
    assert!(post(port, HERO_SELECTION).starts_with("HTTP/1.1 200"));
    let state = listener.state();
    assert_eq!(state.phase, Phase::HeroSelection);
    assert_eq!(state.game_state.as_deref(), Some("DOTA_GAMERULES_STATE_HERO_SELECTION"));
    assert_eq!(state.team, Some(Team::Dire));
    assert_eq!(state.hero_id, Some(2));
    assert_eq!(state.hero_name.as_deref(), Some("npc_dota_hero_axe"));
    assert_eq!(state.match_id.as_deref(), Some("7000000001"));

    // A wrong token, no token, broken JSON and other methods change nothing.
    let wrong = MENU.replace("secret-token", "guess");
    assert!(post(port, &wrong).starts_with("HTTP/1.1 401"));
    assert!(post(port, r#"{"player":{"activity":"menu"}}"#).starts_with("HTTP/1.1 401"));
    assert!(post(port, "not json").starts_with("HTTP/1.1 400"));
    assert!(request(port, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 405"));
    assert_eq!(listener.state().phase, Phase::HeroSelection);

    assert_eq!(*updates.lock().unwrap(), vec![(Phase::Unknown, Phase::Menu), (Phase::Menu, Phase::HeroSelection)]);

    listener.stop();
    assert!(TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(500)).is_err(), "port is freed");
}

#[test]
fn a_used_port_gives_a_clear_message() {
    let first = GsiListener::start(0, "t".into(), None).unwrap();
    let error = GsiListener::start(first.port(), "t".into(), None).err().unwrap();
    assert!(error.contains("already used"), "{error}");
}

#[test]
fn game_states_are_read_like_the_game_names_them() {
    assert_eq!(Phase::from_game_state("DOTA_GAMERULES_STATE_STRATEGY_TIME"), Phase::StrategyTime);
    assert_eq!(Phase::from_game_state("DOTA_GAMERULES_STATE_GAME_IN_PROGRESS"), Phase::Playing);
    assert_eq!(Phase::from_game_state("DOTA_GAMERULES_STATE_PRE_GAME"), Phase::PreGame);
    assert_eq!(Phase::from_game_state("DOTA_GAMERULES_STATE_POST_GAME"), Phase::PostGame);
    assert_eq!(Phase::from_game_state("DOTA_GAMERULES_STATE_WAIT_FOR_PLAYERS_TO_LOAD"), Phase::Loading);
    assert_eq!(Phase::from_game_state("SOMETHING_NEW"), Phase::Unknown);

    // A hero id of 0 (not picked yet) is no hero.
    let json: serde_json::Value = serde_json::from_str(
        r#"{"map":{"game_state":"DOTA_GAMERULES_STATE_HERO_SELECTION","matchid":"0"},"player":{"team_name":"radiant"},"hero":{"id":0,"name":""}}"#,
    )
    .unwrap();
    let state = launch::state_from_json(&json);
    assert_eq!((state.phase, state.team, state.hero_id, state.match_id), (Phase::HeroSelection, Some(Team::Radiant), None, None));
}

// ---- feeding the draft ----------------------------------------------------------------------------

#[test]
fn the_game_tells_the_draft_team_and_hero() {
    let mut draft = DraftState::default();
    draft.allies[0] = Slot { hero_id: Some(1), confidence: 0.9, source: Some(PickSource::Screenshot), alternatives: vec![] };
    let state = GameState { phase: Phase::HeroSelection, team: Some(Team::Dire), hero_id: Some(2), ..Default::default() };

    assert!(launch::apply_to_draft(&mut draft, &state, false));
    assert_eq!(draft.player_team, Some(Team::Dire));
    assert_eq!(draft.player_hero, Some(2));
    // Radiant was assumed: what was on the "allies" side is now the enemy.
    assert_eq!(draft.enemies[0].hero_id, Some(1));
    // The player's own hero (from the game) now sits on the allied side.
    assert!(draft.allies.iter().any(|s| s.hero_id == Some(2) && s.source == Some(PickSource::Gsi)));

    // Nothing new: nothing changes.
    assert!(!launch::apply_to_draft(&mut draft, &state, false));

    // A new match starts a fresh draft but keeps the role.
    draft.role = Some(Role::Mid);
    let next = GameState { phase: Phase::HeroSelection, ..Default::default() };
    assert!(launch::apply_to_draft(&mut draft, &next, true));
    assert_eq!(draft.enemies[0].hero_id, None);
    assert_eq!(draft.role, Some(Role::Mid));
}

// ---- once per launch -----------------------------------------------------------------------------

#[test]
fn the_page_opens_once_per_launch() {
    setup();
    let opened = AtomicUsize::new(0);
    let opener = |url: &str| {
        assert_eq!(url, "https://example.com/players/your-id");
        opened.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };
    let url = "  https://example.com/players/your-id ";

    assert_eq!(launch::open_once_with("1111-1700000000", url, &opener), Ok(OpenOutcome::Opened));
    assert_eq!(launch::open_once_with("1111-1700000000", url, &opener), Ok(OpenOutcome::AlreadyOpened));
    assert_eq!(opened.load(Ordering::SeqCst), 1);

    // The game was started again.
    assert_eq!(launch::open_once_with("2222-1700009999", url, &opener), Ok(OpenOutcome::Opened));
    assert_eq!(opened.load(Ordering::SeqCst), 2);

    // No page set: never opens.
    assert_eq!(launch::open_once_with("3333-1700010000", "", &opener), Ok(OpenOutcome::NoUrl));
    assert_eq!(launch::open_once_with("3333-1700010000", "   ", &opener), Ok(OpenOutcome::NoUrl));
    // Only web addresses.
    assert!(launch::open_once_with("3333-1700010000", "file:///C:/Windows/notepad.exe", &opener).is_err());
    assert!(launch::open_once_with("3333-1700010000", "calc", &opener).is_err());
    assert_eq!(opened.load(Ordering::SeqCst), 2);

    // A failing browser doesn't count as opened.
    let failing = |_: &str| Err("no browser".to_string());
    assert!(launch::open_once_with("4444-1700020000", url, &failing).is_err());
    assert_eq!(launch::open_once_with("4444-1700020000", url, &opener), Ok(OpenOutcome::Opened));
}

#[test]
fn the_menu_is_recognised_with_and_without_game_state_integration() {
    let game = DotaProcess { pid: 42, started: 1_700_000_000 };
    let start = game.started as i64;
    let nothing = GameState::default();
    let menu = GameState { phase: Phase::Menu, last_update: Some(start + 20), ..Default::default() };
    let playing = GameState { phase: Phase::Playing, last_update: Some(start + 20), ..Default::default() };
    let old_menu = GameState { phase: Phase::Menu, last_update: Some(start - 3600), ..Default::default() };

    assert!(!launch::menu_reached(None, &menu, start + 30, 60), "not running");
    assert!(launch::menu_reached(Some(&game), &menu, start + 30, 60));
    assert!(!launch::menu_reached(Some(&game), &playing, start + 600, 60), "in a match, even long after start");
    // No data in this launch (the menu from an earlier launch doesn't count): wait, then assume.
    assert!(!launch::menu_reached(Some(&game), &old_menu, start + 30, 60));
    assert!(!launch::menu_reached(Some(&game), &nothing, start + 59, 60));
    assert!(launch::menu_reached(Some(&game), &nothing, start + 60, 60));
    assert_eq!(game.launch_id(), "42-1700000000");
}

// ---- scripts ---------------------------------------------------------------------------------------

fn run(code: &str) -> localflow_core::lua::engine::ExecutionResult {
    let dir = setup();
    let policy = Arc::new(PathPolicy::new(&[dir.to_path_buf()]));
    execute(code, &RunContext::new(1, "Dota test", "manual", false), policy, Duration::from_secs(20))
}

#[test]
fn scripts_see_the_dota_functions_and_friendly_errors() {
    // No page to open, so a real game running on this PC can't open a browser.
    setup();
    let mut settings = DotaSettings::load();
    settings.launch_url.clear();
    settings.save().unwrap();

    let result = run(
        r#"
        for _, name in ipairs({ "status", "in_menu", "wait_for_menu", "open_launch_url", "capture_draft", "draft",
                                "correct", "reset", "suggest", "build", "heroes", "set_role", "show" }) do
            assert(type(dota[name]) == "function", "missing dota." .. name)
        end
        local s = dota.status()
        assert(type(s.gsi_installed) == "boolean" and type(s.listening) == "boolean", "status fields")
        assert(type(s.state) == "string" and s.source ~= nil, "status state/source")
        assert(s.team == nil, "unknown team is nil, not a placeholder")

        local d = dota.draft()
        assert(#d.allies == 5 and #d.enemies == 5, "ten slots")
        assert(d.allies[1].slot == 1 and d.enemies[5].slot == 5)
        assert(d.team == "radiant" and d.team_assumed == true)

        assert(dota.show() == false, "no desktop app in tests")
        assert(dota.wait_for_menu(0) == false)
        assert(dota.set_role("mid") == true)

        local ok, e = pcall(dota.correct, "sideways", 1, "Axe")
        assert(not ok and tostring(e):find("allies"), tostring(e))
        ok, e = pcall(dota.set_role, "jungler")
        assert(not ok and tostring(e):find("unknown role"), tostring(e))
        -- With or without statistics, a failure is a readable error naming the function.
        for _, call in ipairs({ { dota.suggest, 3 }, { dota.build }, { dota.heroes } }) do
            local ok, e = pcall(call[1], call[2])
            if not ok then log("error: " .. tostring(e)) end
        end
        local opened, why = dota.open_launch_url()
        assert(opened == false and why:find("no page"), why)
        "#,
    );
    assert!(result.success, "{:?}\n{}", result.error, result.output());
    assert_eq!(DotaSettings::load().role, Some(Role::Mid));
    let output = result.output();
    // Errors name the function and read like sentences, whatever the data source does.
    for line in output.lines().filter(|l| l.contains("error: ")) {
        assert!(line.contains("error: dota."), "{line}");
        assert!(!line.contains("stack traceback"), "{line}");
    }
}

#[test]
fn the_item_build_template_explains_what_is_missing() {
    setup();
    let template = localflow_core::lua::find_example("dota-item-build").unwrap();
    let result = run(template.code);
    assert!(result.success, "{:?}\n{}", result.error, result.output());
    let notice = result.logs.iter().find(|l| l.level == "notify").expect("a notification");
    // Without statistics (or without a known hero) it says why instead of failing.
    assert!(!notice.message.contains("runtime error") && !notice.message.contains("traceback"), "{}", notice.message);
    for slug in ["dota-launch-page", "dota-draft-assistant", "dota-item-build"] {
        let example = localflow_core::lua::find_example(slug).unwrap();
        assert_eq!(example.category, "games");
        assert!(!example.allow_system, "{slug} needs no system control");
    }
}
