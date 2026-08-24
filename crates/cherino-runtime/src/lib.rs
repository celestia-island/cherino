//! `cherino-runtime` — OCI-native container runtime backend for `cherino`.
//!
//! This crate wraps [libcontainer](https://github.com/containers/youki) (the
//! library underlying the Youki OCI runtime) as a rootless, daemonless,
//! fallback-tier backend for the [`cherino`](https://crates.io/crates/cherino)
//! container toolkit. On Linux it re-exports the full backend; on every other
//! platform it compiles a stub that returns "only available on Linux" errors,
//! so cross-platform code can depend on it unconditionally.

#![allow(clippy::type_complexity)]

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(not(target_os = "linux"))]
mod stub;

#[cfg(not(target_os = "linux"))]
pub use stub::*;
