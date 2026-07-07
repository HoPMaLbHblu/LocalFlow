use std::{path::Path, sync::Arc, time::Duration};

use localflow::lua::{
    engine::{execute, validate, ExecutionResult, RunContext},
    sandbox::{wildcard_match, PathPolicy},
    EXAMPLES,
};
use tempfile::TempDir;

fn ctx() -> RunContext {
    RunContext {
        automation_id: 7,
        automation_name: "Test".into(),
        trigger: "manual".into(),
    }
}

/// Run `code` with only `allowed` accessible.
fn run_in(allowed: &Path, code: &str) -> ExecutionResult {
    let policy = Arc::new(PathPolicy::new(&[allowed.to_path_buf()]));
    execute(code, &ctx(), policy, Duration::from_secs(5))
}

fn messages(result: &ExecutionResult) -> Vec<String> {
    result.logs.iter().map(|l| l.message.clone()).collect()
}

#[test]
fn validate_accepts_valid_code_and_rejects_syntax_errors() {
    assert!(validate("log('ok')").is_ok());

    let err = validate("if then end").unwrap_err();
    assert!(err.contains("syntax error"), "{err}");
    assert!(err.contains("automation:1"), "{err}");

    assert!(validate("   ").is_err());
}

#[test]
fn all_bundled_examples_compile() {
    for example in EXAMPLES {
        validate(example.code).unwrap_or_else(|e| panic!("{}: {e}", example.slug));
    }
}

#[test]
fn automation_block_runs_with_context() {
    let dir = TempDir::new().unwrap();
    let result = run_in(
        dir.path(),
        r#"
        automation {
            name = "ctx test",
            run = function(ctx)
                log("id=" .. ctx.id .. " name=" .. ctx.name .. " trigger=" .. ctx.trigger)
                notify("done")
            end
        }
        "#,
    );

    assert!(result.success, "{:?}", result.error);
    assert_eq!(messages(&result), ["id=7 name=Test trigger=manual", "done"]);
    assert_eq!(result.logs[1].level, "notify");
    assert!(result.output().contains("[notify] done"));
}

#[test]
fn plain_scripts_run_top_level_code() {
    let dir = TempDir::new().unwrap();
    let result = run_in(dir.path(), "print('a', 1, true)");
    assert!(result.success);
    assert_eq!(messages(&result), ["a\t1\ttrue"]);
}

#[test]
fn automation_without_run_function_fails() {
    let dir = TempDir::new().unwrap();
    let result = run_in(dir.path(), "automation { name = 'x' }");
    assert!(!result.success);
    assert!(result.error.unwrap().contains("run"));
}

#[test]
fn runtime_errors_are_reported_and_logs_are_kept() {
    let dir = TempDir::new().unwrap();
    let result = run_in(dir.path(), "log('before')\nerror('something broke')");
    assert!(!result.success);
    assert_eq!(messages(&result), ["before"]);
    let err = result.error.unwrap();
    assert!(err.contains("something broke"), "{err}");
    assert!(err.contains("automation:2"), "{err}");
}

#[test]
fn dangerous_globals_are_not_available() {
    let dir = TempDir::new().unwrap();
    for code in [
        "os.execute('echo hi')",
        "io.open('x', 'w')",
        "require('os')",
        "load('return 1')()",
        "dofile('x.lua')",
        "debug.getinfo(1)",
        "package.loadlib('x', 'y')",
    ] {
        let result = run_in(dir.path(), code);
        assert!(!result.success, "`{code}` should fail");
    }
}

#[test]
fn infinite_loops_time_out() {
    let dir = TempDir::new().unwrap();
    let policy = Arc::new(PathPolicy::new(&[dir.path().to_path_buf()]));
    let result = execute("while true do end", &ctx(), policy, Duration::from_millis(200));
    assert!(!result.success);
    assert!(result.error.unwrap().contains("timed out"));
}

#[test]
fn fs_list_and_move() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.pdf"), "a").unwrap();
    std::fs::write(root.join("B.PDF"), "b").unwrap();
    std::fs::write(root.join("notes.txt"), "n").unwrap();

    let code = format!(
        r#"
        automation {{
            name = "move pdfs",
            run = function(ctx)
                local files = fs.list([[{root}]], "*.pdf")
                log(#files .. " pdfs")
                for _, file in ipairs(files) do
                    fs.move(file, [[{root}/PDF/]] .. fs.basename(file))
                end
            end
        }}
        "#,
        root = root.display()
    );
    let result = run_in(root, &code);

    assert!(result.success, "{:?}", result.error);
    assert_eq!(messages(&result), ["2 pdfs"]);
    assert!(root.join("PDF/a.pdf").exists());
    assert!(root.join("PDF/B.PDF").exists());
    assert!(!root.join("a.pdf").exists());
    assert!(root.join("notes.txt").exists());
}

#[test]
fn fs_copy_exists_delete_mkdir() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("src.txt"), "hello").unwrap();

    let code = format!(
        r#"
        local root = [[{root}]]
        fs.mkdir(fs.join(root, "out"))
        local copied = fs.copy(fs.join(root, "src.txt"), fs.join(root, "out"))
        log(fs.basename(copied))
        log(tostring(fs.exists(copied)))
        log(tostring(fs.delete(fs.join(root, "src.txt"))))
        log(tostring(fs.delete(fs.join(root, "missing.txt"))))
        log(tostring(fs.exists(fs.join(root, "src.txt"))))
        "#,
        root = root.display()
    );
    let result = run_in(root, &code);

    assert!(result.success, "{:?}", result.error);
    assert_eq!(messages(&result), ["src.txt", "true", "true", "false", "false"]);
    assert_eq!(std::fs::read_to_string(root.join("out/src.txt")).unwrap(), "hello");
}

#[test]
fn fs_move_refuses_to_overwrite() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "a").unwrap();
    std::fs::write(root.join("b.txt"), "b").unwrap();

    let code = format!("fs.move([[{0}/a.txt]], [[{0}/b.txt]])", root.display());
    let result = run_in(root, &code);

    assert!(!result.success);
    assert!(result.error.unwrap().contains("already exists"));
    assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "b");
}

#[test]
fn missing_files_give_clear_errors() {
    let dir = TempDir::new().unwrap();
    let code = format!("fs.move([[{0}/nope.txt]], [[{0}/x.txt]])", dir.path().display());
    let result = run_in(dir.path(), &code);
    assert!(result.error.unwrap().contains("source not found"));

    let code = format!("fs.list([[{0}/no-such-dir]], '*')", dir.path().display());
    let result = run_in(dir.path(), &code);
    assert!(result.error.unwrap().contains("directory not found"));
}

#[test]
fn paths_outside_allowed_directories_are_denied() {
    let allowed = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "s").unwrap();

    // Direct access to another directory.
    let code = format!("fs.exists([[{}/secret.txt]])", outside.path().display());
    let result = run_in(allowed.path(), &code);
    assert!(result.error.unwrap().contains("access denied"));

    // Escaping with `..`.
    let code = format!("fs.list([[{}/../..]], '*')", allowed.path().display());
    let result = run_in(allowed.path(), &code);
    assert!(result.error.unwrap().contains("access denied"));

    // Moving a file out of the sandbox.
    std::fs::write(allowed.path().join("mine.txt"), "m").unwrap();
    let code = format!(
        "fs.move([[{}/mine.txt]], [[{}/stolen.txt]])",
        allowed.path().display(),
        outside.path().display()
    );
    let result = run_in(allowed.path(), &code);
    assert!(result.error.unwrap().contains("access denied"));
    assert!(allowed.path().join("mine.txt").exists());
}

#[test]
fn wildcards() {
    assert!(wildcard_match("*.pdf", "report.pdf"));
    assert!(wildcard_match("*.pdf", "REPORT.PDF"));
    assert!(!wildcard_match("*.pdf", "report.pdf.txt"));
    assert!(wildcard_match("Screenshot*.png", "Screenshot 2024-01-01.png"));
    assert!(wildcard_match("file?.txt", "file1.txt"));
    assert!(!wildcard_match("file?.txt", "file10.txt"));
    assert!(wildcard_match("*", "anything"));
    assert!(wildcard_match("a*b*c", "aXXbYYc"));
}
