//! Zirtola launcher core.
//!
//! Everything that does not need a window lives here so it can be unit-tested,
//! driven from the command line, and reused by the publishing tool:
//!
//! * [`keys`] / [`manifest`]: Ed25519-signed build manifests (the launcher never
//!   installs a byte that is not covered by a signed manifest).
//! * [`token`]: verification of the short-lived launch tokens games receive.
//! * [`api`]: typed client for the Zirtola website API (see `docs/API.md`).
//! * [`install`]: chunked, resumable, parallel install / patch / verify / repair /
//!   uninstall engine.

pub mod api;
pub mod b64;
pub mod install;
pub mod keys;
pub mod manifest;
pub mod token;

pub use keys::TrustedKeys;
pub use manifest::{Chunk, FileEntry, LaunchSpec, Manifest, SignedManifest};
