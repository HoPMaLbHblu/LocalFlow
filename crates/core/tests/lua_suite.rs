//! Runs the Lua test suite in `crates/core/lua_tests/`: every `*_test.lua` file
//! runs as a test automation with an empty folder in `TEST_DIR`.

use std::{path::Path, time::Duration};

use localflow_core::{CoreConfig, LocalFlow};
use tempfile::TempDir;

async fn run_suite(file: &str) {
    let dir = TempDir::new().unwrap();
    // Replaced files go here instead of the real Recycle Bin.
    std::env::set_var("LOCALFLOW_TEST_RECYCLE_DIR", std::env::temp_dir().join("localflow-lua-suite-recycle"));
    let picture = image::RgbImage::from_fn(40, 20, |x, y| image::Rgb([(x * 6) as u8, (y * 12) as u8, 128]));
    picture.save(dir.path().join("picture.png")).unwrap();

    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![dir.path().to_path_buf()],
        script_timeout: Duration::from_secs(60),
    };
    let flow = LocalFlow::open(config, None).await.unwrap();
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("lua_tests").join(file)).unwrap();
    // On the same line as the first line of the file, so error line numbers still match.
    let test_dir = dir.path().to_string_lossy().replace('\\', "/");
    let code = format!("TEST_DIR = {test_dir:?} {source}");

    let result = flow.test_run(code, file.into()).await;
    let log: Vec<String> = result.logs.iter().map(|l| l.message.clone()).collect();
    assert!(result.success, "{file} failed:\n{}\n{}", log.join("\n"), result.error.unwrap_or_default());
    assert!(log.last().is_some_and(|l| l.contains("tests passed")), "{file}: {log:?}");
}

#[tokio::test]
async fn strings() {
    run_suite("strings_test.lua").await;
}

#[tokio::test]
async fn tables() {
    run_suite("tables_test.lua").await;
}

#[tokio::test]
async fn paths_and_dates() {
    run_suite("paths_dates_test.lua").await;
}

#[tokio::test]
async fn helpers() {
    run_suite("helpers_test.lua").await;
}

#[tokio::test]
async fn media() {
    run_suite("media_test.lua").await;
}

/// Every file in lua_tests/ must have a test above.
#[test]
fn every_suite_is_listed() {
    let listed = ["strings_test.lua", "tables_test.lua", "paths_dates_test.lua", "helpers_test.lua", "media_test.lua"];
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("lua_tests");
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(listed.contains(&name.as_str()), "add a test for {name} in tests/lua_suite.rs");
    }
}
