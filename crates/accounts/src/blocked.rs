//! The operations that are blocked on a missing capture
//! (`docs/missing-captures.md` §1).
//!
//! `validate`, `switch_account`, `start_keepalive`,
//! `list_identities`, and `select_identity` all need the account-menu
//! request/response shape, which is not in the fixtures: no request
//! fixture mentions `account`, and `dataSyncId` appears nowhere in
//! `fixtures/redacted/`. Nothing here is guessed — the functions in
//! [`SessionManager`](crate::SessionManager) return
//! [`Error::MissingCapture`] until the capture lands.
//!
//! The types below are the shapes the task spec names (name, handle,
//! thumbnail, channel ID for an account; a data-sync id per brand
//! identity). Their fields come from the spec text, not from a
//! fixture, so no response is parsed into them yet.

use serde::{Deserialize, Serialize};

use crate::Error;

/// The signed-in account, as the account-menu response would describe
/// it. No parser exists for this yet — the response shape is
/// unconfirmed until the capture in `docs/missing-captures.md` §1
/// lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    /// Display name.
    pub name: String,
    /// Handle (`@...`), when the response carries one.
    pub handle: Option<String>,
    /// Avatar thumbnail URL, when the response carries one.
    pub thumbnail: Option<String>,
    /// The account's channel id (`UC…`).
    pub channel_id: String,
}

/// One brand account under the same Google sign-in, as the
/// identities response would describe it. Same caveat as
/// [`AccountInfo`]: no parser exists yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// The delegated identity for `context.user.onBehalfOfUser`.
    pub data_sync_id: String,
    /// Display name of the brand account.
    pub name: String,
}

/// The error every blocked operation returns. `what` names the
/// operation and the capture it needs.
pub(crate) fn missing_capture(what: &'static str) -> Error {
    Error::MissingCapture(what)
}
