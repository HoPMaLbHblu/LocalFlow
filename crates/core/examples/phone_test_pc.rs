//! A throwaway "PC" for testing the phone app: an in-memory LocalFlow with two sample automations,
//! connected to a relay, that approves every pairing request automatically.
//! Prints `QR <text>` once it is ready. For tests only — never point this at real data.
//!   cargo run -p localflow-core --example phone_test_pc -- ws://127.0.0.1:8787

use std::sync::Arc;
use std::time::Duration;

use localflow_core::phone::crypto::{b64, random};
use localflow_core::phone::handler::PcInfo;
use localflow_core::phone::link::{Link, LinkEvent, PcSecrets, PhoneConfig};
use localflow_core::{AutomationInput, CoreConfig, LocalFlow};

#[tokio::main]
async fn main() {
    let relay = std::env::args().nth(1).unwrap_or_else(|| "ws://127.0.0.1:8787".into());
    let dir = tempfile::tempdir().unwrap();
    let flow = LocalFlow::open(
        CoreConfig { database_url: "sqlite::memory:".into(), allowed_dirs: vec![dir.path().to_path_buf()], script_timeout: Duration::from_secs(5) },
        None,
    )
    .await
    .unwrap();
    for (name, code) in [("Say hello", "log('hello from the phone')"), ("Tidy downloads", "log('tidied ' .. (ctx.folder or 'Downloads'))")] {
        flow.create(&AutomationInput { name: name.into(), lua_code: code.into(), enabled: true, ..Default::default() }).await.unwrap();
    }
    PhoneConfig { enabled: true, relay, pc_id: b64(&random::<16>()), phones: vec![] }.save(dir.path()).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let link = Link::start(
        flow,
        dir.path().to_path_buf(),
        PcSecrets::generate(),
        PcInfo { name: "Test PC".into(), version: "test".into() },
        Arc::new(move |e| {
            if let LinkEvent::PairRequest { device, .. } = e {
                let _ = tx.send(device);
            }
        }),
    );
    while !link.state.lock().unwrap().online {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("QR {}", link.start_pairing().await.unwrap());
    while let Some(device) = rx.recv().await {
        link.allow(&device);
        println!("ALLOWED {device}");
    }
}
