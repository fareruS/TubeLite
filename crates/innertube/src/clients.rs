//! Client identities: which YouTube client a request pretends to be.
//!
//! Loaded from the embedded `clients.json` (captured values only —
//! currently just WEB, the client in every fixture) with an optional
//! override file. The override wins entirely when it exists; that is
//! how a new client version is rolled out without rebuilding.
//!
//! The WEB entry is the captured ground truth:
//! - `clientName` `WEB`, `clientVersion` `2.20261112.10.00`
//!   (from the captured requests; the old `2.20250101.00.00` in
//!   `yt-source/clients.json` is a stale placeholder).
//! - `clientId` `"1"` — the numeric id the browser sends as
//!   `x-youtube-client-name` (verified in the captured request).
//! - `userAgent` the browser's UA from the capture.
//! - `acceptLanguage` `en-US,en;q=0.9,id;q=0.8` from the capture.
//! - `osName`/`osVersion` `Windows`/`10.0`, `platform` `DESKTOP`.
//!
//! ANDROID/IOS exist in the old `yt-source/clients.json` with
//! registry-verified versions, but no ANDROID/IOS request was ever
//! captured, so they are deliberately NOT in this file. Add them
//! through the override file once a capture exists.

use std::path::Path;

use serde::Deserialize;

use crate::Error;

/// One client identity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Client {
    /// `clientName`, e.g. `WEB`.
    #[serde(rename = "clientName")]
    pub name: String,
    /// `clientVersion`, e.g. `2.20261112.10.00`.
    #[serde(rename = "clientVersion")]
    pub version: String,
    /// `clientId` as a string holding a number, e.g. `"1"`. Sent as
    /// the `x-youtube-client-name` header.
    #[serde(rename = "clientId")]
    pub id: String,
    /// The `User-Agent` header.
    #[serde(rename = "userAgent")]
    pub user_agent: String,
    /// The `Accept-Language` header, when the client defines one.
    #[serde(rename = "acceptLanguage", default)]
    pub accept_language: Option<String>,
    /// `osName` from the capture, e.g. `Windows`.
    #[serde(rename = "osName", default)]
    pub os_name: Option<String>,
    /// `osVersion` from the capture, e.g. `10.0`.
    #[serde(rename = "osVersion", default)]
    pub os_version: Option<String>,
    /// Device make, when known.
    #[serde(rename = "deviceMake", default)]
    pub device_make: Option<String>,
    /// Device model, when known.
    #[serde(rename = "deviceModel", default)]
    pub device_model: Option<String>,
    /// Platform, e.g. `DESKTOP`.
    #[serde(default)]
    pub platform: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClientsFile {
    clients: Vec<Client>,
}

/// The loaded client set.
#[derive(Debug, Clone)]
pub struct Clients {
    clients: Vec<Client>,
}

impl Clients {
    /// Load from the embedded `clients.json`.
    pub fn embedded() -> Result<Self, Error> {
        Self::from_json(include_str!("clients.json"))
    }

    /// Load from an override file when `path` is `Some` and the file
    /// exists; otherwise fall back to the embedded `clients.json`.
    /// The override wins entirely — it is not merged with the bundle.
    pub fn load(override_path: Option<&Path>) -> Result<Self, Error> {
        if let Some(path) = override_path {
            if path.exists() {
                let text = std::fs::read_to_string(path)?;
                return Self::from_json(&text);
            }
        }
        Self::embedded()
    }

    fn from_json(text: &str) -> Result<Self, Error> {
        let file: ClientsFile = serde_json::from_str(text)?;
        if file.clients.is_empty() {
            return Err(Error::InvalidResponse(
                "clients.json has no clients".to_string(),
            ));
        }
        Ok(Self {
            clients: file.clients,
        })
    }

    /// Find a client by `clientName`.
    pub fn get(&self, name: &str) -> Option<&Client> {
        self.clients.iter().find(|c| c.name == name)
    }

    /// All loaded clients, in file order.
    pub fn all(&self) -> &[Client] {
        &self.clients
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_web_is_the_captured_version() {
        let clients = Clients::embedded().expect("embedded clients.json");
        let web = clients.get("WEB").expect("WEB client");
        assert_eq!(web.version, "2.20261112.10.00");
        assert_eq!(web.id, "1");
        assert!(web.user_agent.contains("Chrome/157.0.0.0"));
    }

    #[test]
    fn override_file_wins_over_the_bundle() {
        let dir = std::env::temp_dir().join("innertube-clients-override-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("clients.json");
        std::fs::write(
            &path,
            r#"{"clients":[{"clientName":"WEB","clientVersion":"9.9.9.9.99.99","clientId":"1","userAgent":"TestAgent/1.0"}]}"#,
        )
        .expect("write override");
        let clients = Clients::load(Some(&path)).expect("load override");
        assert_eq!(clients.get("WEB").expect("WEB").version, "9.9.9.9.99.99");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_override_falls_back_to_embedded() {
        let clients =
            Clients::load(Some(Path::new("/nonexistent/clients.json"))).expect("fallback");
        assert_eq!(clients.get("WEB").expect("WEB").version, "2.20261112.10.00");
    }
}
