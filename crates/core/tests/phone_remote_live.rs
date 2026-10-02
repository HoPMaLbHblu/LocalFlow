//! End to end through a real relay: pairing, handshake, approval, requests, permissions.
//! Needs the LocalFlow Remote relay running locally (`npx wrangler dev` in its relay folder):
//!   LF_TEST_RELAY=ws://127.0.0.1:8787 cargo test -p localflow-core --test phone_remote_live -- --ignored

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use localflow_core::phone::crypto::{b64, random, unb64, unb64_32, KeyPair, Session, Side};
use localflow_core::phone::handler::PcInfo;
use localflow_core::phone::link::{Link, LinkEvent, PcSecrets, PhoneConfig};
use localflow_core::{AutomationInput, CoreConfig, LocalFlow};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

fn query(qr: &str, key: &str) -> String {
    let q = qr.split_once('?').unwrap().1;
    let raw = q.split('&').find_map(|kv| kv.strip_prefix(&format!("{key}="))).unwrap().to_string();
    // minimal percent-decoding for the relay URL
    let mut out = Vec::new();
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(&raw[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

#[tokio::test]
#[ignore = "needs a local relay (LF_TEST_RELAY)"]
async fn a_phone_pairs_and_controls_localflow() {
    let relay = std::env::var("LF_TEST_RELAY").expect("LF_TEST_RELAY");
    let dir = tempfile::tempdir().unwrap();
    let flow = LocalFlow::open(
        CoreConfig { database_url: "sqlite::memory:".into(), allowed_dirs: vec![dir.path().to_path_buf()], script_timeout: Duration::from_secs(5) },
        None,
    )
    .await
    .unwrap();
    let auto = flow
        .create(&AutomationInput { name: "Say hello".into(), lua_code: "log('hello from the phone')".into(), enabled: true, ..Default::default() })
        .await
        .unwrap();

    PhoneConfig { enabled: true, relay: relay.clone(), pc_id: b64(&random::<16>()), phones: vec![] }.save(dir.path()).unwrap();
    let seen = Arc::new(Mutex::new(Vec::<LinkEvent>::new()));
    let seen2 = seen.clone();
    let link = Link::start(
        flow.clone(),
        dir.path().to_path_buf(),
        PcSecrets::generate(),
        PcInfo { name: "Test PC".into(), version: "test".into() },
        Arc::new(move |e| seen2.lock().unwrap().push(e)),
    );
    for _ in 0..50 {
        if link.state.lock().unwrap().online { break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(link.state.lock().unwrap().online, "link reached the relay");

    // --- the phone scans the QR code
    let qr = link.start_pairing().await.unwrap();
    assert!(qr.starts_with("lfremote://pair?v=1&"));
    let (pc_id, pc_key, secret) = (query(&qr, "pc"), unb64_32(&query(&qr, "key")).unwrap(), query(&qr, "s"));
    let device = b64(&random::<16>());
    let device_token = b64(&random::<32>());
    let phone = KeyPair::generate();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut req = format!("{relay}/v1/pc/{pc_id}/phone?device={device}&pair={secret}").into_client_request().unwrap();
    req.headers_mut().insert("authorization", format!("Bearer {device_token}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.expect("phone connects with the QR secret");

    async fn next_data<S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin>(ws: &mut S) -> String {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timeout").unwrap().unwrap();
            if let Message::Text(t) = msg {
                let v: Value = serde_json::from_str(&t).unwrap();
                if v["t"] == "data" {
                    return v["d"].as_str().unwrap().to_string();
                }
            }
        }
    }

    // --- handshake
    let eph = KeyPair::generate();
    let hello = json!({ "h": 1, "e": b64(&eph.public), "s": b64(&phone.public), "n": "Pixel test" });
    ws.send(Message::Text(json!({ "t": "data", "d": b64(hello.to_string().as_bytes()) }).to_string().into())).await.unwrap();
    let reply: Value = serde_json::from_slice(&unb64(&next_data(&mut ws).await).unwrap()).unwrap();
    let pc_eph = unb64_32(reply["e"].as_str().unwrap()).unwrap();
    let mut session = Session::derive(Side::Phone, &phone.secret, &pc_key, &eph.secret, &pc_eph, &unb64(&pc_id).unwrap(), &unb64(&device).unwrap()).unwrap();

    // --- before the user allows it, requests are ignored
    let frame = session.seal(json!({ "id": 1, "m": "automations.list" }).to_string().as_bytes());
    ws.send(Message::Text(json!({ "t": "data", "d": frame }).to_string().into())).await.unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(800), next_data(&mut ws)).await.is_err(), "no answer before approval");

    // --- the PC asks the user; the user allows it
    let asked = seen.lock().unwrap().iter().any(|e| matches!(e, LinkEvent::PairRequest { name, .. } if name == "Pixel test"));
    assert!(asked, "the PC was asked to allow the phone");
    link.allow(&device);
    let paired: Value = serde_json::from_slice(&session.open(&next_data(&mut ws).await).unwrap()).unwrap();
    assert_eq!(paired["ev"], "paired");

    let mut call = |id: u64, m: &str, p: Value| {
        let f = session.seal(json!({ "id": id, "m": m, "p": p }).to_string().as_bytes());
        json!({ "t": "data", "d": f }).to_string()
    };
    ws.send(Message::Text(call(2, "automations.list", json!({})).into())).await.unwrap();
    let f2 = next_data(&mut ws).await;
    ws.send(Message::Text(call(3, "automations.run", json!({ "id": auto.id })).into())).await.unwrap();
    let f3 = next_data(&mut ws).await;
    ws.send(Message::Text(call(4, "system.lock", json!({})).into())).await.unwrap();
    let f4 = next_data(&mut ws).await;

    let list: Value = serde_json::from_slice(&session.open(&f2).unwrap()).unwrap();
    assert_eq!(list["ok"], true);
    assert_eq!(list["r"][0]["name"], "Say hello");
    let run: Value = serde_json::from_slice(&session.open(&f3).unwrap()).unwrap();
    assert_eq!(run["r"]["started"], true);
    let lock: Value = serde_json::from_slice(&session.open(&f4).unwrap()).unwrap();
    assert_eq!(lock["ok"], false, "power actions are off by default");
    assert_eq!(lock["e"]["code"], "forbidden");

    // The run really happened.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let runs = flow.runs(auto.id, 5).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, "success");

    // --- revoking disconnects the phone
    link.revoke(&device);
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "revoked phone was disconnected");
    assert!(PhoneConfig::load(dir.path()).phones.is_empty());
    link.stop();
}
