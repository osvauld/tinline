use super::*;
use std::collections::HashSet;
async fn tags(hub: &BlobHub) -> HashSet<String> {
    let mut stream = Box::pin(hub.store.tags().list().await.unwrap());
    let mut names = HashSet::new();
    while let Some(info) = stream.next().await {
        names.insert(String::from_utf8(info.unwrap().name.0.to_vec()).unwrap());
    }
    names
}
#[tokio::test]
async fn profile_deletion_and_orphan_recovery_preserve_chat_and_other_owner_claims() {
    let dir = std::env::temp_dir().join(format!("profile-blob-tags-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let hub = BlobHub::open(
        &dir,
        Arc::new(|_, _| false),
        Arc::new(|_, _, _, _| {}),
        Arc::new(|_, _, _| {}),
    )
    .await
    .unwrap();
    let hash = hub
        .add_profile_bytes("owner-a", [3; 32], vec![7; 100])
        .await
        .unwrap();
    let claim = hub.profile_claim("owner-a", &hash).await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(30),
            hub.profile_claim("owner-a", &hash)
        )
        .await
        .is_err(),
        "same-owner readiness and cleanup are serialized"
    );
    let other = tokio::time::timeout(
        Duration::from_millis(100),
        hub.profile_claim("owner-b", &hash),
    )
    .await
    .unwrap();
    drop(other);
    drop(claim);
    hub.keep_profile("owner-b", &hash).await.unwrap();
    hub.keep(&hash).await.unwrap();
    let a = BlobHub::protected_tag(Some("owner-a"), &hash);
    let b = BlobHub::protected_tag(Some("owner-b"), &hash);
    let chat = tag_of(&hash);
    hub.release_profile("owner-a", &hash).await.unwrap();
    let have = tags(&hub).await;
    assert!(!have.contains(&a));
    assert!(have.contains(&b));
    assert!(have.contains(&chat));
    hub.keep_profile("owner-a", &hash).await.unwrap();
    hub.recover_profile_tags(&HashSet::from([b.clone()]))
        .await
        .unwrap();
    let have = tags(&hub).await;
    assert!(
        !have.contains(&a),
        "uncommitted/deleted profile claim is dropped on reopen"
    );
    assert!(have.contains(&b), "another owner's retained copy survives");
    assert!(
        have.contains(&chat),
        "chat media is outside the permit scope"
    );
    assert!(hub.complete_size(&hash).await.is_some());
    hub.shutdown().await;
    std::fs::remove_dir_all(dir).unwrap();
}
