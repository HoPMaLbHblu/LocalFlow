//! Runs the file and media templates for real, in a fake home folder with
//! sample files, and checks what they produced.

use std::{fs, path::Path, time::Duration};

use localflow_core::{lua::find_example, CoreConfig, LocalFlow, TestRunResult};
use tempfile::TempDir;

fn write(home: &Path, path: &str, contents: &[u8]) {
    let file = home.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, contents).unwrap();
}

fn picture(home: &Path, path: &str, width: u32, height: u32) {
    let file = home.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(width, height, |x, y| image::Rgb([x as u8, y as u8, 90])).save(file).unwrap();
}

async fn run(flow: &LocalFlow, slug: &str) -> TestRunResult {
    let example = find_example(slug).unwrap_or_else(|| panic!("no template {slug}"));
    let result = flow.test_run(example.code.to_string(), example.title.into()).await;
    let log: Vec<_> = result.logs.iter().map(|l| l.message.as_str()).collect();
    assert!(result.success, "{slug} failed: {:?}\n{}", result.error, log.join("\n"));
    result
}

fn messages(result: &TestRunResult) -> String {
    result.logs.iter().map(|l| l.message.clone()).collect::<Vec<_>>().join("\n")
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

// One test, because the fake home folder is set for the whole process.
#[tokio::test]
async fn file_and_media_templates_work_end_to_end() {
    let home_dir = TempDir::new().unwrap();
    let home = home_dir.path().canonicalize().unwrap();
    std::env::set_var("LOCALFLOW_TEST_HOME", &home);
    std::env::set_var("LOCALFLOW_TEST_RECYCLE_DIR", home.join("_recycle"));
    let config = CoreConfig {
        database_url: "sqlite::memory:".into(),
        allowed_dirs: vec![home.clone()],
        script_timeout: Duration::from_secs(60),
    };
    let flow = LocalFlow::open(config, None).await.unwrap();

    // Downloads with duplicates, old temp files, and a webp.
    write(&home, "Downloads/report.pdf", b"same bytes here");
    write(&home, "Downloads/report (1).pdf", b"same bytes here");
    write(&home, "Downloads/notes.txt", b"unique");
    write(&home, "Downloads/old.tmp", b"leftover");
    let week_ago = std::time::SystemTime::now() - Duration::from_secs(10 * 24 * 3600);
    fs::File::options().write(true).open(home.join("Downloads/old.tmp")).unwrap().set_modified(week_ago).unwrap();
    write(&home, "Downloads/fresh.part", b"still downloading");
    picture(&home, "Downloads/tmp-pic.png", 8, 8);
    let img = image::open(home.join("Downloads/tmp-pic.png")).unwrap();
    img.save(home.join("Downloads/cat.webp")).unwrap();
    fs::remove_file(home.join("Downloads/tmp-pic.png")).unwrap();

    let r = run(&flow, "find-duplicates").await;
    assert!(messages(&r).contains("1 sets of duplicates"), "{}", messages(&r));
    assert_eq!(files_in(&home.join("Documents/LocalFlow reports")).len(), 1);

    run(&flow, "webp-to-png").await;
    assert!(home.join("Downloads/cat.png").exists());

    run(&flow, "clean-temp-files").await;
    assert!(!home.join("Downloads/old.tmp").exists(), "old temp file should be recycled");
    assert!(home.join("Downloads/fresh.part").exists(), "new temp files stay");
    assert!(home.join("_recycle/old.tmp").exists());

    let r = run(&flow, "downloads-report").await;
    assert!(messages(&r).contains("Downloads report saved"));
    let reports = files_in(&home.join("Documents/LocalFlow reports"));
    assert!(reports.iter().any(|n| n.ends_with(".csv")), "{reports:?}");
    let csv_name = reports.iter().find(|n| n.starts_with("downloads") && n.ends_with(".csv")).unwrap();
    let csv = fs::read_to_string(home.join("Documents/LocalFlow reports").join(csv_name)).unwrap();
    assert!(csv.starts_with("name,type,bytes,modified\n") && csv.contains("notes.txt,txt,6,"), "{csv}");

    // Photos: shrink, and sort by date (no camera data, so the file date is used).
    picture(&home, "Pictures/To share/beach.png", 3200, 1600);
    run(&flow, "shrink-photos").await;
    let small = image::open(home.join("Pictures/To share/small/beach.jpg")).unwrap();
    assert_eq!((small.width(), small.height()), (1600, 800));

    picture(&home, "Pictures/Unsorted/IMG_0001.png", 4, 4);
    run(&flow, "sort-photos-by-date").await;
    assert!(files_in(&home.join("Pictures/Unsorted")).is_empty());
    let year = chrono::Local::now().format("%Y").to_string();
    assert_eq!(files_in(&home.join("Pictures/Photos")), vec![year]);

    // Journal, scans, backups, spending, birthdays.
    let r = run(&flow, "daily-journal").await;
    assert!(messages(&r).contains("Journal page ready"));
    run(&flow, "daily-journal").await; // a second run leaves the page alone
    let journal = fs::read_dir(home.join("Documents/Journal")).unwrap().count();
    assert_eq!(journal, 1);

    write(&home, "Documents/Scans/scan0042.pdf", b"%PDF");
    run(&flow, "date-in-scan-names").await;
    let scans = files_in(&home.join("Documents/Scans"));
    assert_eq!(scans.len(), 1);
    assert!(scans[0].ends_with(" scan0042.pdf") && scans[0].as_bytes()[4] == b'-', "{scans:?}");

    for n in 1..=9 {
        write(&home, &format!("Backups/backup-{n}.zip"), b"zip");
        let when = std::time::SystemTime::now() - Duration::from_secs(3600 * (10 - n));
        fs::File::options().write(true).open(home.join(format!("Backups/backup-{n}.zip"))).unwrap().set_modified(when).unwrap();
    }
    run(&flow, "backup-rotation").await;
    let backups = files_in(&home.join("Backups"));
    assert_eq!(backups.len(), 7);
    assert!(!backups.contains(&"backup-1.zip".to_string()) && !backups.contains(&"backup-2.zip".to_string()));

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    write(
        &home,
        "Documents/expenses.csv",
        format!("date,category,amount,note\n{today},food,10.50,a\n{today},Food,\"2,25\",b\n{today},rent,500,c\nbad,x,y,z\n").as_bytes(),
    );
    let r = run(&flow, "monthly-expenses").await;
    let out = messages(&r);
    assert!(out.contains("Total this month: 512.75"), "{out}");
    assert!(out.contains("Skipped line 5"), "{out}");

    let md = chrono::Local::now().format("%m-%d").to_string();
    write(&home, "Documents/birthdays.csv", format!("name,birthday\nSam,1990-{md}\nNo date,soon\n").as_bytes());
    let r = run(&flow, "birthday-reminders").await;
    let out = messages(&r);
    assert!(out.contains("Sam turns") && out.contains("today"), "{out}");
    assert!(out.contains("Skipped No date"), "{out}");

    // Monitoring templates work before any history exists.
    run(&flow, "pc-health").await;
    run(&flow, "system-history").await;

    // The lock template refuses to run with the example password.
    let lock = find_example("lock-private-files").unwrap();
    let r = flow.test_run(lock.code.to_string(), "lock".into()).await;
    assert!(r.error.unwrap().contains("choose your own password"));
}
