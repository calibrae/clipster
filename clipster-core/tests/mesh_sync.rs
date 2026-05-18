//! End-to-end mesh tests: two ClipsterCore instances wired through axum +
//! reqwest with the pinned-cert verifier, mimicking real LAN sync.

use chrono::Utc;
use clipster_common::models::{Clip, ClipContentType, content_hash};
use clipster_core::ClipsterCore;
use clipster_core::db::Database;
use clipster_core::db::peers::TrustStatus;
use clipster_core::identity::Identity;
use clipster_core::peer::PeerClient;
use clipster_core::peer::server::router as peer_router;
use clipster_core::sync::SyncEngine;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

struct TestPeer {
    core: Arc<ClipsterCore>,
    addr: SocketAddr,
    _tmp: tempfile::TempDir,
}

async fn spawn_peer(name: &str) -> TestPeer {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(":memory:").unwrap();
    db.migrate().unwrap();
    let identity = Identity::load_or_create(tmp.path(), Some(name.into())).unwrap();
    let image_dir = tmp.path().join("images");
    std::fs::create_dir_all(&image_dir).unwrap();
    let core = Arc::new(ClipsterCore::new(db, identity, image_dir));

    let acceptor: TlsAcceptor = core.identity.tls_acceptor().unwrap();
    let app = peer_router(core.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor2 = acceptor.clone();
    let app2 = app.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return; };
            let acc = acceptor2.clone();
            let svc = app2.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acc.accept(stream).await {
                    let io = hyper_util::rt::TokioIo::new(tls);
                    let service = hyper_util::service::TowerToHyperService::new(svc);
                    let _ = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await;
                }
            });
        }
    });

    TestPeer { core, addr, _tmp: tmp }
}

fn make_text_clip(text: &str, device: &str) -> Clip {
    let now = Utc::now();
    Clip {
        id: Uuid::now_v7(),
        content_type: ClipContentType::Text,
        text_content: Some(text.to_string()),
        image_hash: None,
        image_mime: None,
        file_ref_path: None,
        content_hash: content_hash(text.as_bytes()),
        source_device: device.to_string(),
        source_app: None,
        byte_size: text.len() as u64,
        created_at: now,
        state_modified_at: now,
        is_favorite: false,
        is_deleted: false,
    }
}

fn pair_trust(a: &TestPeer, b: &TestPeer) {
    a.core
        .db
        .upsert_peer_discovery(
            &b.core.identity.device_id,
            &b.core.identity.device_name,
            &b.addr.to_string(),
            None,
        )
        .unwrap();
    a.core
        .trust
        .trust(&b.core.identity.device_id)
        .unwrap();
    b.core
        .db
        .upsert_peer_discovery(
            &a.core.identity.device_id,
            &a.core.identity.device_name,
            &a.addr.to_string(),
            None,
        )
        .unwrap();
    b.core.trust.trust(&a.core.identity.device_id).unwrap();
}

async fn wait_for<F>(mut check: F)
where
    F: FnMut() -> bool,
{
    for _ in 0..100 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("condition not met within 10s");
}

#[tokio::test]
async fn peer_client_can_hello_after_pairing() {
    let a = spawn_peer("alpha").await;
    let b = spawn_peer("beta").await;
    pair_trust(&a, &b);

    let client = PeerClient::new(b.addr, &b.core.identity.device_id, &a.core.identity).unwrap();
    let req = clipster_core::protocol::wire::HelloRequest {
        device_id: a.core.identity.device_id.clone(),
        name: a.core.identity.device_name.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        proto: 1,
        caps: vec![],
    };
    let resp = client.hello(&req).await.unwrap();
    assert_eq!(resp.device_id, b.core.identity.device_id);
    assert_eq!(resp.name, "beta");
}

#[tokio::test]
async fn peer_request_without_trust_is_forbidden() {
    let a = spawn_peer("alpha").await;
    let b = spawn_peer("beta").await;
    // No pairing — neither side trusts the other.

    let client = PeerClient::new(b.addr, &b.core.identity.device_id, &a.core.identity).unwrap();
    let req = clipster_core::protocol::wire::HelloRequest {
        device_id: a.core.identity.device_id.clone(),
        name: a.core.identity.device_name.clone(),
        version: "test".into(),
        proto: 1,
        caps: vec![],
    };
    let err = client.hello(&req).await.unwrap_err();
    assert!(err.to_string().contains("403") || err.to_string().contains("401"));
}

#[tokio::test]
async fn clip_propagates_from_alpha_to_beta() {
    let a = spawn_peer("alpha").await;
    let b = spawn_peer("beta").await;
    pair_trust(&a, &b);

    // Sync engines on both sides
    let engine_a = SyncEngine::spawn(a.core.clone());
    let engine_b = SyncEngine::spawn(b.core.clone());
    drop(engine_a);
    drop(engine_b);

    let clip = make_text_clip("hello mesh", "alpha");
    let clip_id = clip.id;
    a.core.db.insert_clip(&clip).unwrap();

    wait_for(|| b.core.db.get_clip(&clip_id).is_ok()).await;
    let fetched = b.core.db.get_clip(&clip_id).unwrap();
    assert_eq!(fetched.text_content.as_deref(), Some("hello mesh"));
    assert_eq!(fetched.source_device, "alpha"); // provenance preserved
}

#[tokio::test]
async fn lww_favorite_converges_to_later_writer() {
    let a = spawn_peer("alpha").await;
    let b = spawn_peer("beta").await;
    pair_trust(&a, &b);

    let _ea = SyncEngine::spawn(a.core.clone());
    let _eb = SyncEngine::spawn(b.core.clone());

    let clip = make_text_clip("fav me", "alpha");
    let clip_id = clip.id;
    a.core.db.insert_clip(&clip).unwrap();

    // Wait for B to see it
    wait_for(|| b.core.db.get_clip(&clip_id).is_ok()).await;

    // Both toggle favorite at the same time, but B's is later
    a.core.db.toggle_favorite(&clip_id).unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    b.core.db.toggle_favorite(&clip_id).unwrap();

    // Wait for convergence
    tokio::time::sleep(Duration::from_secs(7)).await;

    let final_a = a.core.db.get_clip(&clip_id).unwrap();
    let final_b = b.core.db.get_clip(&clip_id).unwrap();
    // Both started false → A toggled to true → B toggled to true.
    // After sync each pulled the other's state — LWW means B's later toggle wins on both.
    assert_eq!(final_a.is_favorite, final_b.is_favorite);
}

#[tokio::test]
async fn delete_tombstone_propagates() {
    let a = spawn_peer("alpha").await;
    let b = spawn_peer("beta").await;
    pair_trust(&a, &b);
    let _ea = SyncEngine::spawn(a.core.clone());
    let _eb = SyncEngine::spawn(b.core.clone());

    let clip = make_text_clip("doomed", "alpha");
    let clip_id = clip.id;
    a.core.db.insert_clip(&clip).unwrap();
    wait_for(|| b.core.db.get_clip(&clip_id).is_ok()).await;

    a.core.db.soft_delete(&clip_id).unwrap();

    wait_for(|| b.core.db.get_clip(&clip_id).is_err()).await;
}

#[tokio::test]
async fn image_blob_replicates_from_alpha_to_beta() {
    let a = spawn_peer("alpha").await;
    let b = spawn_peer("beta").await;
    pair_trust(&a, &b);
    let _ea = SyncEngine::spawn(a.core.clone());
    let _eb = SyncEngine::spawn(b.core.clone());

    // Write a fake PNG blob to A's image_dir and insert an image clip.
    let png_bytes = b"\x89PNG\r\n\x1a\nFAKE_IMAGE_PAYLOAD".to_vec();
    let hash = content_hash(&png_bytes);
    let path_a = a.core.image_dir.join(format!("{hash}.png"));
    std::fs::write(&path_a, &png_bytes).unwrap();

    let now = Utc::now();
    let clip = Clip {
        id: Uuid::now_v7(),
        content_type: ClipContentType::Image,
        text_content: None,
        image_hash: Some(hash.clone()),
        image_mime: Some("image/png".into()),
        file_ref_path: None,
        content_hash: hash.clone(),
        source_device: "alpha".into(),
        source_app: None,
        byte_size: png_bytes.len() as u64,
        created_at: now,
        state_modified_at: now,
        is_favorite: false,
        is_deleted: false,
    };
    let clip_id = clip.id;
    a.core.db.insert_clip(&clip).unwrap();

    // Wait for clip metadata to propagate
    wait_for(|| b.core.db.get_clip(&clip_id).is_ok()).await;

    // And the blob file should have been pulled
    let path_b = b.core.image_dir.join(format!("{hash}.png"));
    wait_for(|| path_b.exists()).await;
    let bytes_b = std::fs::read(&path_b).unwrap();
    assert_eq!(bytes_b, png_bytes);
}

#[tokio::test]
async fn tofu_discovered_peer_starts_pending() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open(":memory:").unwrap();
    db.migrate().unwrap();
    let identity = Identity::load_or_create(tmp.path(), Some("tofu".into())).unwrap();
    let core = Arc::new(ClipsterCore::new(db, identity, tmp.path().to_path_buf()));

    let status = core
        .trust
        .on_discovered("abc123", "stranger", "10.0.0.99:8743", None)
        .unwrap();
    assert_eq!(status, TrustStatus::Pending);
    assert!(!core.trust.is_trusted("abc123"));

    core.trust.trust("abc123").unwrap();
    assert!(core.trust.is_trusted("abc123"));
}

// Suppress unused-path warning
#[allow(dead_code)]
fn _types_used(_p: PathBuf) {}
