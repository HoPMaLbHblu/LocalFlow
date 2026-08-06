//! Checks every code example in the in-app guide. The examples live in the
//! TypeScript app, so this runs only when given a JSON export of them:
//!
//! ```text
//! LOCALFLOW_GUIDE_EXAMPLES=guide_examples.json cargo test -p localflow-core --test guide_examples -- --ignored
//! ```
//!
//! Every example must compile. Examples are also run in an empty home folder;
//! failures that only mean "that file doesn't exist here" or "needs system
//! control" are expected, anything else is reported.

use std::time::Duration;

use localflow_core::{lua::engine::validate, CoreConfig, LocalFlow};
use serde_json::Value;
use tempfile::TempDir;

/// Errors that are fine for an example run in an empty folder.
const EXPECTED: &[&str] = &[
    "not found",
    "does not exist",
    "no automation named",
    "Allow system control",
    "outside the allowed",
    "could not find an app",
    "choose your own password",
    "http.",
    "cannot find the path",
    "The system cannot find",
];

/// Examples that act on the real PC (open apps, press keys, ask questions) are only compiled.
const NOT_RUN: &[&str] = &[
    "app.open", "ask(", "shell.", "keyboard.", "mouse.", "window.", "process.kill", "system.lock", "system.sleep",
    "system.shutdown", "system.restart", "system.mute", "system.volume", "system.brightness", "system.set_wallpaper",
    "system.wake_at", "network.", "http.", "clipboard.set", "sound.", "wait(", "ai.ask", "ai.chat",
];

#[tokio::test]
#[ignore = "needs LOCALFLOW_GUIDE_EXAMPLES"]
async fn every_guide_example_works() {
    let Some(file) = std::env::var_os("LOCALFLOW_GUIDE_EXAMPLES") else { return };
    let examples: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();

    let home = TempDir::new().unwrap();
    let home_path = home.path().canonicalize().unwrap();
    std::env::set_var("LOCALFLOW_TEST_HOME", &home_path);
    std::env::set_var("LOCALFLOW_TEST_RECYCLE_DIR", home_path.join("_recycle"));
    for folder in ["Downloads", "Documents/Notes", "Pictures", "Desktop"] {
        std::fs::create_dir_all(home_path.join(folder)).unwrap();
    }
    let flow = LocalFlow::open(
        CoreConfig {
            database_url: "sqlite::memory:".into(),
            allowed_dirs: vec![home_path.clone()],
            script_timeout: Duration::from_secs(20),
        },
        None,
    )
    .await
    .unwrap();

    let mut problems = Vec::new();
    let mut ran = 0;
    for example in &examples {
        let from = example["from"].as_str().unwrap();
        let code = example["code"].as_str().unwrap();
        if let Err(e) = validate(code) {
            problems.push(format!("{from}: does not compile: {e}"));
            continue;
        }
        // Snippets are pieces for inside your own code (they use variables like `file`), so they are only compiled.
        if from.starts_with("snippet") || !example["runnable"].as_bool().unwrap_or(true) || NOT_RUN.iter().any(|n| code.contains(n)) {
            continue;
        }
        ran += 1;
        let result = flow.test_run(code.to_string(), "guide".into()).await;
        if let Some(error) = result.error {
            if !EXPECTED.iter().any(|e| error.contains(e)) {
                problems.push(format!("{from}: {error}\n    code: {}", code.replace('\n', "\n          ")));
            }
        }
    }
    println!("{} examples compiled, {ran} ran", examples.len());
    assert!(problems.is_empty(), "{} problem(s):\n{}", problems.len(), problems.join("\n"));
}
