use localflow::db::{self, models::NewAutomation, repository::Repository};

async fn repo() -> Repository {
    Repository::new(db::connect("sqlite::memory:").await.expect("database"))
}

fn sample(name: &str) -> NewAutomation {
    NewAutomation {
        name: name.to_string(),
        description: "test automation".to_string(),
        lua_code: "log('hi')".to_string(),
        schedule: None,
        enabled: true,
    }
}

#[tokio::test]
async fn create_read_update_delete() {
    let repo = repo().await;

    let created = repo.create_automation(&sample("First")).await.unwrap();
    assert_eq!(created.name, "First");
    assert!(created.enabled);
    assert_eq!(created.schedule, None);

    let fetched = repo.get_automation(created.id).await.unwrap().unwrap();
    assert_eq!(fetched.lua_code, "log('hi')");

    let mut changes = sample("Renamed");
    changes.schedule = Some("0 * * * * *".into());
    changes.enabled = false;
    let updated = repo.update_automation(created.id, &changes).await.unwrap().unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.schedule.as_deref(), Some("0 * * * * *"));
    assert!(!updated.enabled);

    assert!(repo.delete_automation(created.id).await.unwrap());
    assert!(repo.get_automation(created.id).await.unwrap().is_none());
    assert!(!repo.delete_automation(created.id).await.unwrap());
}

#[tokio::test]
async fn update_missing_automation_returns_none() {
    let repo = repo().await;
    assert!(repo.update_automation(999, &sample("x")).await.unwrap().is_none());
}

#[tokio::test]
async fn list_is_sorted_by_name() {
    let repo = repo().await;
    repo.create_automation(&sample("banana")).await.unwrap();
    repo.create_automation(&sample("Apple")).await.unwrap();

    let names: Vec<_> = repo.list_automations().await.unwrap().into_iter().map(|a| a.name).collect();
    assert_eq!(names, ["Apple", "banana"]);
}

#[tokio::test]
async fn set_enabled_toggles() {
    let repo = repo().await;
    let a = repo.create_automation(&sample("Toggle me")).await.unwrap();

    let a = repo.set_enabled(a.id, false).await.unwrap().unwrap();
    assert!(!a.enabled);
    let a = repo.set_enabled(a.id, true).await.unwrap().unwrap();
    assert!(a.enabled);
}

#[tokio::test]
async fn runs_are_recorded_newest_first() {
    let repo = repo().await;
    let a = repo.create_automation(&sample("Runner")).await.unwrap();

    let first = repo.start_run(a.id).await.unwrap();
    repo.finish_run(first, "success", "[info] ok", None).await.unwrap();
    let second = repo.start_run(a.id).await.unwrap();
    repo.finish_run(second, "failed", "", Some("boom")).await.unwrap();

    let runs = repo.list_runs(a.id, 10).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].id, second);
    assert_eq!(runs[0].status, "failed");
    assert_eq!(runs[0].error.as_deref(), Some("boom"));
    assert!(runs[0].finished_at.is_some());
    assert!(runs[0].duration_ms().unwrap() >= 0);
    assert_eq!(runs[1].output.as_deref(), Some("[info] ok"));

    assert_eq!(repo.latest_run(a.id).await.unwrap().unwrap().id, second);
}

#[tokio::test]
async fn interrupted_runs_are_marked_failed() {
    let repo = repo().await;
    let a = repo.create_automation(&sample("Interrupted")).await.unwrap();
    let run_id = repo.start_run(a.id).await.unwrap();

    assert_eq!(repo.fail_interrupted_runs().await.unwrap(), 1);
    let run = repo.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(run.status, "failed");
    assert!(run.error.unwrap().contains("Interrupted"));
}

#[tokio::test]
async fn logs_are_stored_newest_first() {
    let repo = repo().await;
    let a = repo.create_automation(&sample("Logger")).await.unwrap();

    repo.add_log(a.id, "info", "one").await.unwrap();
    repo.add_log(a.id, "error", "two").await.unwrap();

    let logs = repo.list_logs(a.id, 10).await.unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].message, "two");
    assert_eq!(logs[0].level, "error");
    assert_eq!(logs[1].message, "one");
}

#[tokio::test]
async fn deleting_an_automation_removes_its_runs_and_logs() {
    let repo = repo().await;
    let a = repo.create_automation(&sample("Doomed")).await.unwrap();
    let run_id = repo.start_run(a.id).await.unwrap();
    repo.add_log(a.id, "info", "hello").await.unwrap();

    repo.delete_automation(a.id).await.unwrap();

    assert!(repo.get_run(run_id).await.unwrap().is_none());
    assert!(repo.list_logs(a.id, 10).await.unwrap().is_empty());
}
