//! SessionManager behaviour with a fake login flow and an in-memory
//! secret store — no webview, no keyring, no network, no Tauri.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use accounts::{
    AccountStore, CookieSet, Error, LoginFlow, MemorySecretStore, SecretStore, SessionManager,
};
use innertube::{Session, SharedSession};

fn cookies() -> CookieSet {
    CookieSet::from_pairs([
        ("SAPISID".to_string(), "abc".to_string()),
        ("VISITOR_INFO1_LIVE".to_string(), "v1".to_string()),
    ])
}

/// A flow whose two methods return fixed answers.
struct FakeFlow {
    silent: Result<CookieSet, String>,
    interactive: Result<CookieSet, String>,
}

impl LoginFlow for FakeFlow {
    fn interactive_login(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>> {
        let result = self.interactive.clone().map_err(Error::Flow);
        Box::pin(async move { result })
    }

    fn silent_refresh(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>> {
        let result = self.silent.clone().map_err(Error::Flow);
        Box::pin(async move { result })
    }
}

fn manager(flow: FakeFlow) -> (Arc<SessionManager>, Arc<MemorySecretStore>) {
    let store = Arc::new(Mutex::new(AccountStore::open_in_memory().unwrap()));
    let secrets = Arc::new(MemorySecretStore::new());
    let session = SharedSession::new(Session::default());
    let manager = Arc::new(SessionManager::new(
        store,
        Arc::clone(&secrets) as Arc<dyn accounts::SecretStore>,
        session,
        Arc::new(flow),
    ));
    (manager, secrets)
}

#[tokio::test]
async fn add_account_from_raw_header_stores_and_activates() {
    let (manager, secrets) = manager(FakeFlow {
        silent: Err("unused".into()),
        interactive: Err("unused".into()),
    });
    let status = manager
        .add_account("SAPISID=abc; VISITOR_INFO1_LIVE=v1")
        .await
        .unwrap();
    assert!(status.signed_in);
    // md5("abc") = 900150983cd24fb0d6963f7d28e17f72.
    assert_eq!(
        status.account_id.as_deref(),
        Some("ga-900150983cd24fb0d6963f7d28e17f72")
    );
    assert_eq!(manager.list_accounts().unwrap().len(), 1);
    let stored = secrets
        .get("ga-900150983cd24fb0d6963f7d28e17f72")
        .unwrap()
        .unwrap();
    assert_eq!(stored.expose(), "SAPISID=abc; VISITOR_INFO1_LIVE=v1");
    assert!(manager.session().snapshot().await.signed_in());
}

#[tokio::test]
async fn add_account_from_netscape_keeps_only_youtube_and_google() {
    let (manager, secrets) = manager(FakeFlow {
        silent: Err("unused".into()),
        interactive: Err("unused".into()),
    });
    let text = "# Netscape HTTP Cookie File\n\
        .example.com\tTRUE\t/\tTRUE\t0\tSID\tother\n\
        .google.com\tTRUE\t/\tTRUE\t0\tSID\tgoog\n\
        .youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tabc\n";
    manager.add_account(text).await.unwrap();
    let stored = secrets
        .get("ga-900150983cd24fb0d6963f7d28e17f72")
        .unwrap()
        .unwrap();
    assert!(stored.expose().contains("SID=goog"));
    assert!(!stored.expose().contains("SID=other"));
}

#[tokio::test]
async fn add_account_rejects_cookies_without_sapisid() {
    let (manager, _) = manager(FakeFlow {
        silent: Err("unused".into()),
        interactive: Err("unused".into()),
    });
    let err = manager
        .add_account("VISITOR_INFO1_LIVE=v1; PREF=hl=en")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotSignedIn));
    assert!(manager.list_accounts().unwrap().is_empty());
    assert!(!manager.status().await.signed_in);
}

#[tokio::test]
async fn heal_prefers_silent_refresh() {
    let (manager, secrets) = manager(FakeFlow {
        silent: Ok(cookies()),
        interactive: Err("should not be called".into()),
    });
    let callback = manager.heal_callback();
    callback(manager.session().clone()).await;

    let snapshot = manager.session().snapshot().await;
    assert!(snapshot.signed_in());
    assert_eq!(
        manager.status().await.account_id.as_deref(),
        Some("ga-900150983cd24fb0d6963f7d28e17f72")
    );
    assert!(secrets
        .get("ga-900150983cd24fb0d6963f7d28e17f72")
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn heal_falls_back_to_the_popup_when_silent_fails() {
    let (manager, _) = manager(FakeFlow {
        silent: Err("webview session expired".into()),
        interactive: Ok(CookieSet::from_pairs([(
            "SAPISID".to_string(),
            "popup".to_string(),
        )])),
    });
    let callback = manager.heal_callback();
    callback(manager.session().clone()).await;

    let snapshot = manager.session().snapshot().await;
    assert_eq!(snapshot.sapisid().as_deref(), Some("popup"));
}

#[tokio::test]
async fn heal_leaves_a_dead_session_alone_when_both_fail() {
    let (manager, _) = manager(FakeFlow {
        silent: Err("no".into()),
        interactive: Err("cancelled".into()),
    });
    let callback = manager.heal_callback();
    callback(manager.session().clone()).await;

    assert!(!manager.session().snapshot().await.signed_in());
    assert!(manager.list_accounts().unwrap().is_empty());
}

#[tokio::test]
async fn load_active_restores_the_saved_session_and_sign_out_clears_it() {
    let store = Arc::new(Mutex::new(AccountStore::open_in_memory().unwrap()));
    let secrets: Arc<dyn accounts::SecretStore> = Arc::new(MemorySecretStore::new());
    let flow: Arc<dyn LoginFlow> = Arc::new(FakeFlow {
        silent: Err("unused".into()),
        interactive: Err("unused".into()),
    });
    let manager = Arc::new(SessionManager::new(
        Arc::clone(&store),
        Arc::clone(&secrets),
        SharedSession::new(Session::default()),
        Arc::clone(&flow),
    ));
    manager.add_account_cookies(&cookies()).await.unwrap();
    assert!(manager.status().await.signed_in);

    // A fresh manager over the same stores restores it.
    let manager2 = Arc::new(SessionManager::new(
        Arc::clone(&store),
        Arc::clone(&secrets),
        SharedSession::new(Session::default()),
        flow,
    ));
    assert!(!manager2.session().snapshot().await.signed_in());
    assert!(manager2.load_active().await.unwrap());
    assert!(manager2.session().snapshot().await.signed_in());

    // Sign-out clears the session and the active pointer, but the
    // stored account and its secret stay (so the user can come back).
    let status = manager2.sign_out().await.unwrap();
    assert!(!status.signed_in);
    assert_eq!(status.account_id, None);
    assert!(!manager2.session().snapshot().await.signed_in());
    assert_eq!(manager2.list_accounts().unwrap().len(), 1);
    assert!(secrets
        .get("ga-900150983cd24fb0d6963f7d28e17f72")
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn remove_account_deletes_the_secret_and_clears_the_session() {
    let (manager, secrets) = manager(FakeFlow {
        silent: Err("unused".into()),
        interactive: Err("unused".into()),
    });
    manager.add_account("SAPISID=abc").await.unwrap();
    manager.add_account("SAPISID=def").await.unwrap();
    assert_eq!(manager.list_accounts().unwrap().len(), 2);
    // The second add is active now.
    let active = manager.status().await.account_id.unwrap();
    manager.remove_account(&active).await.unwrap();
    assert_eq!(manager.list_accounts().unwrap().len(), 1);
    assert!(secrets.get(&active).unwrap().is_none());
    assert!(!manager.session().snapshot().await.signed_in());
    assert_eq!(manager.status().await.account_id, None);
    // Removing an unknown id is a no-op, not an error.
    manager.remove_account("ga-nope").await.unwrap();
}

#[tokio::test]
async fn switch_account_is_blocked_and_leaves_the_active_account_unchanged() {
    let (manager, _) = manager(FakeFlow {
        silent: Err("unused".into()),
        interactive: Err("unused".into()),
    });
    manager.add_account("SAPISID=abc").await.unwrap();
    let before = manager.status().await;
    assert!(before.signed_in);

    // Refused — and the active account is untouched, whether the id
    // exists or not (the operation cannot validate yet).
    for id in ["ga-nope", "ga-900150983cd24fb0d6963f7d28e17f72"] {
        let err = manager.switch_account(id).await.unwrap_err();
        assert!(matches!(err, Error::MissingCapture(_)));
        assert!(err.to_string().contains("docs/missing-captures.md"));
    }
    assert_eq!(manager.status().await, before);
}

#[tokio::test]
async fn blocked_operations_return_missing_capture() {
    let (manager, _) = manager(FakeFlow {
        silent: Err("unused".into()),
        interactive: Err("unused".into()),
    });
    assert!(matches!(
        manager.validate(&cookies()).await.unwrap_err(),
        Error::MissingCapture(_)
    ));
    assert!(matches!(
        manager.start_keepalive(std::time::Duration::from_secs(1800)),
        Err(Error::MissingCapture(_))
    ));
    assert!(matches!(
        manager.list_identities().await.unwrap_err(),
        Error::MissingCapture(_)
    ));
    assert!(matches!(
        manager.select_identity("sync-id").await.unwrap_err(),
        Error::MissingCapture(_)
    ));
}
