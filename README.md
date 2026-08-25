<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/master/res/logo/cherino.webp" alt="Cherino" width="240" /></p>

<h1 align="center">Cherino</h1>

<p align="center"><strong>Unified, runtime-agnostic container operations toolkit for the celestia platform</strong></p>

<div align="center">

[![License: SySL-1.0](https://img.shields.io/badge/License-SySL--1.0-blue.svg)](https://sysl.celestia.world)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fcherino-blue.svg)](https://github.com/celestia-island/cherino)
[![Docs](https://img.shields.io/badge/docs-cherino.docs.celestia.world-blue)](https://cherino.docs.celestia.world)
[![docs.rs](https://docs.rs/cherino/badge.svg)](https://docs.rs/cherino)
[![Checks](https://img.shields.io/github/actions/workflow/status/celestia-island/cherino/checks.yml)](https://github.com/celestia-island/cherino/actions/workflows/checks.yml)

</div>

<div align="center">

**English** ·
[简体中文](./docs/zhs/README.md) ·
[繁體中文](./docs/zht/README.md) ·
[日本語](./docs/ja/README.md) ·
[한국어](./docs/ko/README.md) ·
[Français](./docs/fr/README.md) ·
[Español](./docs/es/README.md) ·
[Русский](./docs/ru/README.md) ·
[العربية](./docs/ar/README.md)

</div>

Cherino is a container operations toolkit for the celestia platform — a
standalone [Rust](https://www.rust-lang.org/) library providing a unified,
runtime-agnostic API for creating and managing sandboxed containers.

`cherino` defines the [`ContainerOps`] trait — the full container lifecycle
(create, start, stop, exec, file copy, snapshots, volumes, images) — plus a
reference Docker implementation and a shared security-profile layer (seccomp,
AppArmor, Landlock, egress control, registry whitelisting). `cherino-runtime`
adds an OCI-native rootless backend built on
[libcontainer](https://github.com/containers/youki).

## Crate layout

| Crate | Description |
|-------|-------------|
| [`cherino-macros`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-macros) | `Getters` derive macro used by the DTO types |
| [`cherino`](https://github.com/celestia-island/cherino/tree/master/crates/cherino) | `ContainerOps` trait, Docker backend, security profiles, shared types |
| [`cherino-runtime`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-runtime) | Youki/libcontainer OCI backend (Linux-only, non-Linux stubs) |

## ContainerOps backends

| Backend | Type | Platform | Mechanism | Tier |
|---------|------|----------|-----------|------|
| **Docker** | API | All | Bollard → Docker Engine HTTP API | primary |
| **Youki** | Native | Linux | libcontainer → OCI rootless containers | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell-out | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI (VM-per-container) | fallback |

**Youki is fallback-tier**: the libcontainer backend in `cherino-runtime` is
maintained as a rootless daemonless alternative for hosts without Docker, not
as the primary orchestration driver. The Docker backend is the default and the
most battle-tested path.

## Features

`cherino`:

- `docker` *(default)* — the Bollard-based `ContainerManager` backend.
- `cli-backend` — the WSLc / Apple Container CLI adapters (`cli-backend`
  module). Off by default; enable explicitly on Windows/macOS hosts.
- `docker-tests` — integration tests that require a live Docker daemon
  (off by default; implies `docker`).

`cherino-runtime` builds its Linux backend automatically on
`cfg(target_os = "linux")`; all other platforms compile a stub that returns
"only available on Linux" errors.

## Quick start

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

```rust
use cherino::{ContainerManager, ops::ContainerOps};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = ContainerManager::new()?; // connects to the local Docker daemon
    let infos = mgr.list().await?;
    for info in infos {
        println!("{}: {:?}", info.name(), info.status());
    }
    Ok(())
}
```

Loading the FUSE AppArmor profile required by nested containers (host, root,
once per host):

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Hosts that still carry the legacy `celestia-plana-fuse` profile are detected
and accepted with a deprecation warning; install the new name when you can.

## Branding and compatibility

`cherino` was extracted from the `plana` workspace. The following legacy names
are still read for compatibility, each emitting a `tracing::warn!`:

| Legacy (plana / entelecheia) | New (cherino) |
|------------------------------|---------------|
| AppArmor profile `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` env | `CHERINO_APPARMOR_UNCONFINED` env |
| `ENTELECHEIA_RUN_DIR` env | `CHERINO_RUN_DIR` env |
| `/tmp/entelecheia/youki` run dir | `/tmp/cherino/youki` run dir |

The generic overrides (`CONTAINER_RUN_DIR`, `CONTAINER_ROOTFS_URL`,
`CONTAINER_NETWORK`) keep precedence over all branded names.

## AI-generated disclosure

This repository's code is substantially AI-generated and is licensed under the
[SySL-1.0](LICENSE) license. See the `sysl` repository
(<https://github.com/celestia-island/sysl>) for the license text, the model
disclosure appended to this repo's [LICENSE](LICENSE), and the FAQ.

## License

Licensed under the [SySL-1.0](LICENSE) license. By using this software you
accept its AI-generation disclosure and risk-acknowledgment terms.
