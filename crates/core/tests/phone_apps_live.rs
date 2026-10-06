//! Read-only: lists the real open windows the way "Close all apps" sees them (closes nothing).
//!   cargo test -p localflow-core --test phone_apps_live -- --ignored --nocapture
use std::collections::BTreeSet;
use std::time::Duration;

use localflow_core::phone::handler::{handle, PcInfo, Permission, Request};
use localflow_core::{CoreConfig, LocalFlow};

#[tokio::test]
#[ignore = "reads this PC's open windows"]
async fn list_open_apps() {
    let dir = tempfile::tempdir().unwrap();
    let flow = LocalFlow::open(CoreConfig { database_url: "sqlite::memory:".into(), allowed_dirs: vec![dir.path().into()], script_timeout: Duration::from_secs(10) }, None).await.unwrap();
    let perms: BTreeSet<Permission> = [Permission::Power].into();
    let info = PcInfo { name: "test".into(), version: "test".into() };
    let r = handle(&flow, &perms, &info, serde_json::from_str::<Request>(r#"{"id":1,"m":"apps.open"}"#).unwrap()).await;
    assert_eq!(r["ok"], true, "{r}");
    for w in r["r"].as_array().unwrap() {
        println!("{} {:<28} {}", if w["kept"] == true { "KEEP " } else { "close" }, w["app"].as_str().unwrap(), w["title"].as_str().unwrap().chars().take(50).collect::<String>());
    }
}
