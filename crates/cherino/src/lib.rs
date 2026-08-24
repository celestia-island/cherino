//! Container abstraction layer for sandboxed execution — the `cherino` crate.
//!
//! Every sandboxed workload in the celestia platform runs inside an ephemeral,
//! sandboxed container. This crate provides the unified API for managing those
//! containers, whether the underlying runtime is Docker (via Bollard, the
//! default `docker` feature), Youki (via the `cherino-runtime` crate), or a
//! CLI-based runtime like WSL Containers or Apple Container (the opt-in
//! `cli-backend` feature). Downstream code only depends on the
//! [`ContainerOps`] trait, never on a concrete runtime.
//!
//! ## Runtime Backends
//!
//! | Backend | Type | Platform | Mechanism | Tier |
//! |---------|------|----------|-----------|------|
//! | **Docker** | API | All | Bollard → Docker Engine HTTP API | primary |
//! | **Youki** | Native | Linux | libcontainer → OCI rootless containers | fallback |
//! | **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell-out | fallback |
//! | **Apple Container** | CLI | macOS 26+ | `container` CLI (VM-per-container) | fallback |
//!
//! The Youki/libcontainer backend (`cherino-runtime`) is maintained as a
//! fallback-tier runtime for hosts without a Docker daemon; Docker remains the
//! primary, most battle-tested driver.
//!
//! ## Two-Layer Runtime Architecture
//!
//! The platform uses two distinct container runtimes at different layers:
//!
//! | Layer | Runtime | Default | Used by |
//! |-------|---------|---------|---------|
//! | **Outer** (orchestration) | Docker/Podman | `CONTAINER_RUNTIME=docker` | TUI health check, scepter daemon, server manager |
//! | **Inner** (cosmos sandbox) | Youki/libcontainer | `COSMOS_CONTAINER_RUNTIME=youki` | Snowflake manager, Neikos agent state |
//!
//! The outer layer manages the infrastructure containers (scepter, postgres) via the
//! Docker/Podman API. These containers require full orchestration features: networking,
//! persistent volumes, health checks, and multi-container composition.
//!
//! The inner layer (Cosmos) runs *inside* the scepter container and uses youki to create
//! lightweight, fast-start sandboxed containers for agent execution. Each agent gets its
//! own isolated container with seccomp profiles and resource limits.
//!
//! Runtime selection helpers live in the consumer services
//! (`outer_runtime_type()`, `cosmos_runtime_type()`).
//!
//! Architecture:
//! - **Trait** — [`ContainerOps`] defines the full lifecycle: create, start, stop, remove,
//!   exec, copy files in/out, snapshot filesystem changes, manage volumes, and pull images.
//! - **Docker reference implementation** — [`ContainerManager`] uses `bollard::Docker` to
//!   drive the Docker Engine API (behind the default `docker` feature).
//! - **Security** — [`SeccompProfile`], [`LandlockRules`], [`EgressPolicy`], and
//!   [`RegistryWhitelist`] enforce defence-in-depth: syscall filtering, network egress
//!   control, and allowed image registries. These are shared with the Youki backend so
//!   both runtimes enforce the same policy.
//! - **Types** — shared DTOs ([`ContainerCreateParams`], [`ExecOutput`], [`ContainerInfo`],
//!   etc.) used by both runtimes and serialized across the JSON-RPC bridge.
//!
//! The design treats container sandboxing as a *policy-over-mechanism* problem: the
//! platform prescribes *what* is allowed (via security profiles), and each runtime
//! translates that into its native mechanism (Docker host configs / OCI spec hooks).
#![allow(clippy::type_complexity)]

pub mod apparmor;
pub mod binding;
#[cfg(feature = "cli-backend")]
pub mod cli_backend;
#[cfg(feature = "docker")]
pub mod conversion;
#[cfg(feature = "docker")]
pub mod copy_ops;
#[cfg(feature = "docker")]
pub mod docker_client;
pub mod egress;
pub mod errors;
pub mod events;
#[cfg(feature = "docker")]
pub mod exec_ops;
#[cfg(feature = "docker")]
pub mod image_ops;
pub mod landlock;
#[cfg(feature = "docker")]
pub mod lifecycle;
#[cfg(feature = "docker")]
pub mod manager;
pub mod ops;
pub mod registry_whitelist;
pub mod seccomp;
pub mod security_profile;
pub mod toolchain;
pub mod types;
#[cfg(feature = "docker")]
pub mod volume_ops;

pub use apparmor::{
    FUSE_PROFILE, FUSE_PROFILE_NAME, LEGACY_FUSE_PROFILE_NAME, LEGACY_UNCONFINED_ENV,
    UNCONFINED_ENV, fuse_security_opts, installed_profile_name, is_apparmor_unconfined,
};
pub use binding::{BindingId, ContainerBindResult, ContainerBinding};
#[cfg(feature = "cli-backend")]
pub use cli_backend::CliContainerBackend;
#[cfg(feature = "docker")]
pub use copy_ops::{
    DiffHomeChange, HomeChange, changed_home_paths, changed_home_paths_from_diff,
    changed_home_paths_with_kind, changed_home_paths_with_kind_from_diff,
    changed_paths_from_diff_in_prefixes, changed_paths_in_prefixes,
    changed_paths_with_kind_from_diff_in_prefixes, changed_paths_with_kind_in_prefixes,
    filter_tar_to_changed_paths,
};
pub use egress::{EgressMode, EgressPolicy, EgressRule};
pub use errors::ContainerError;
pub use events::ContainerEvent;
pub use landlock::LandlockRules;
#[cfg(feature = "docker")]
pub use manager::ContainerManager;
pub use ops::ContainerOps;
pub use registry_whitelist::{RegistryEntry, RegistryWhitelist};
pub use seccomp::{SeccompProfile, SeccompProfileData, build_security_opts};
#[cfg(feature = "docker")]
pub use security_profile::apply_to_host_config;
pub use security_profile::{ContainerSecurity, compile, cosmos, postgres, scepter};
pub use toolchain::{
    LspServerEntry, ToolchainImageProfile, find_for_language, find_lsp_profile, list_profiles,
    load_profile, parse_profile,
};
pub use types::{
    ChangeKind, ContainerCreateParams, ContainerDetail, ContainerForkParams, ContainerInfo,
    ContainerRuntimeType, ContainerStatus, DockerVolumeInfo, ExecOutput, HealthcheckParams,
    ImageInfo, PathChange, ServerStatus, VolumeMount, WritableRootfs,
};
