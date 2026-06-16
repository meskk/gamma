//! Round-trip test against the local MinIO: presigned PUT → head → presigned GET.
//! Proves bytes go straight to the object store over HTTP, never through us.
//! Requires the `minio` service from docker-compose to be running.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use storage::{Storage, StorageConfig};

fn test_storage() -> Storage {
    Storage::new(StorageConfig {
        endpoint: "http://localhost:9000".into(),
        region: "us-east-1".into(),
        bucket: "gamma-media-test".into(),
        access_key: "gamma".into(),
        secret_key: "gammasecret".into(),
    })
}

fn unique_key() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("test/{nanos}.txt")
}

#[tokio::test]
async fn presigned_put_head_get_roundtrip() {
    let store = test_storage();
    store.ensure_bucket().await.expect("ensure bucket");

    let key = unique_key();
    let body = b"hello gamma media";
    let ttl = Duration::from_secs(60);

    // Not there yet.
    assert!(store.head(&key).await.unwrap().is_none());

    // Upload directly to the store via the presigned PUT URL.
    let put_url = store
        .presign_put(&key, "text/plain", ttl)
        .await
        .expect("presign put");
    let resp = reqwest::Client::new()
        .put(&put_url)
        .header("content-type", "text/plain")
        .body(body.to_vec())
        .send()
        .await
        .expect("upload");
    assert!(
        resp.status().is_success(),
        "upload failed: {}",
        resp.status()
    );

    // Now it exists with the right size.
    assert_eq!(store.head(&key).await.unwrap(), Some(body.len() as i64));

    // Download directly via the presigned GET URL.
    let get_url = store.presign_get(&key, ttl).await.expect("presign get");
    let got = reqwest::get(&get_url)
        .await
        .expect("download")
        .bytes()
        .await
        .expect("body");
    assert_eq!(&got[..], body);
}
