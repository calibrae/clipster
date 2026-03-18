use clipster_server::db::Database;
use clipster_server::state::AppState;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::net::TcpListener;

async fn spawn_test_server() -> String {
    let db = Database::open(":memory:").unwrap();
    db.migrate().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let image_dir = tmp.path().to_str().unwrap().to_string();

    let state = AppState::new(db, image_dir, None);
    let app = clipster_server::routes::router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // Keep tmp alive so the dir isn't deleted
        let _tmp = tmp;
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    format!("http://{addr}")
}

fn client() -> Client {
    Client::new()
}

fn create_clip_body(text: &str, device: &str) -> Value {
    json!({
        "text_content": text,
        "source_device": device
    })
}

#[tokio::test]
async fn health_returns_ok() {
    let base = spawn_test_server().await;
    let resp = client()
        .get(format!("{base}/api/v1/health"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn create_text_clip_returns_201() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("hello world", "test-device"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["content_type"], "text");
    assert_eq!(body["text_content"], "hello world");
    assert_eq!(body["source_device"], "test-device");
    assert_eq!(body["is_favorite"], false);
    assert_eq!(body["is_deleted"], false);
    assert!(body["id"].is_string());
    assert!(body["content_hash"].is_string());
}

#[tokio::test]
async fn list_clips_returns_created_clips() {
    let base = spawn_test_server().await;
    let c = client();

    c.post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("clip one", "dev1"))
        .send()
        .await
        .unwrap();

    c.post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("clip two", "dev2"))
        .send()
        .await
        .unwrap();

    let resp = c
        .get(format!("{base}/api/v1/clips"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["total_count"], 2);
    let clips = body["clips"].as_array().unwrap();
    assert_eq!(clips.len(), 2);
}

#[tokio::test]
async fn get_clip_by_id() {
    let base = spawn_test_server().await;
    let c = client();

    let create_resp = c
        .post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("find me", "dev"))
        .send()
        .await
        .unwrap();
    let created: Value = create_resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    let resp = c
        .get(format!("{base}/api/v1/clips/{id}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"], id);
    assert_eq!(body["text_content"], "find me");
}

#[tokio::test]
async fn delete_clip_soft_deletes() {
    let base = spawn_test_server().await;
    let c = client();

    let create_resp = c
        .post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("delete me", "dev"))
        .send()
        .await
        .unwrap();
    let created: Value = create_resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    // Delete
    let del_resp = c
        .delete(format!("{base}/api/v1/clips/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), 204);

    // GET by ID should 404
    let get_resp = c
        .get(format!("{base}/api/v1/clips/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 404);

    // List should not include it
    let list_resp = c
        .get(format!("{base}/api/v1/clips"))
        .send()
        .await
        .unwrap();
    let body: Value = list_resp.json().await.unwrap();
    assert_eq!(body["total_count"], 0);
}

#[tokio::test]
async fn toggle_favorite_twice() {
    let base = spawn_test_server().await;
    let c = client();

    let create_resp = c
        .post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("fav me", "dev"))
        .send()
        .await
        .unwrap();
    let created: Value = create_resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    // First toggle: false -> true
    let resp = c
        .patch(format!("{base}/api/v1/clips/{id}/favorite"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_favorite"], true);

    // Second toggle: true -> false
    let resp = c
        .patch(format!("{base}/api/v1/clips/{id}/favorite"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_favorite"], false);
}

#[tokio::test]
async fn duplicate_within_5s_returns_409() {
    let base = spawn_test_server().await;
    let c = client();

    let body = create_clip_body("same content", "dev");

    let resp1 = c
        .post(format!("{base}/api/v1/clips"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), 201);

    let resp2 = c
        .post(format!("{base}/api/v1/clips"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 409);
}

#[tokio::test]
async fn search_filter_works() {
    let base = spawn_test_server().await;
    let c = client();

    c.post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("rust programming language", "dev"))
        .send()
        .await
        .unwrap();

    c.post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("python scripting", "dev"))
        .send()
        .await
        .unwrap();

    let resp = c
        .get(format!("{base}/api/v1/clips?search=rust"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["total_count"], 1);
    let clips = body["clips"].as_array().unwrap();
    assert_eq!(clips[0]["text_content"], "rust programming language");
}

#[tokio::test]
async fn pagination_limit_offset() {
    let base = spawn_test_server().await;
    let c = client();

    for i in 0..5 {
        c.post(format!("{base}/api/v1/clips"))
            .json(&create_clip_body(&format!("clip number {i}"), "dev"))
            .send()
            .await
            .unwrap();
    }

    // First page
    let resp = c
        .get(format!("{base}/api/v1/clips?limit=2&offset=0"))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["total_count"], 5);
    let page1 = body["clips"].as_array().unwrap();
    assert_eq!(page1.len(), 2);

    // Second page
    let resp = c
        .get(format!("{base}/api/v1/clips?limit=2&offset=2"))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["total_count"], 5);
    let page2 = body["clips"].as_array().unwrap();
    assert_eq!(page2.len(), 2);

    // Pages should not overlap
    assert_ne!(page1[0]["id"], page2[0]["id"]);
    assert_ne!(page1[1]["id"], page2[1]["id"]);
}

#[tokio::test]
async fn get_clip_content_returns_plain_text() {
    let base = spawn_test_server().await;
    let c = client();

    let create_resp = c
        .post(format!("{base}/api/v1/clips"))
        .json(&create_clip_body("raw content here", "dev"))
        .send()
        .await
        .unwrap();
    let created: Value = create_resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    let resp = c
        .get(format!("{base}/api/v1/clips/{id}/content"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(content_type.contains("text/plain"));
    let text = resp.text().await.unwrap();
    assert_eq!(text, "raw content here");
}
