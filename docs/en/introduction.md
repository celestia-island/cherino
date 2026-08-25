# Introduction

Cherino is the container operations toolkit of the celestia platform: a
standalone Rust library that provides a unified, runtime-agnostic API for
creating and managing sandboxed containers.

Every sandboxed workload in the celestia platform runs inside an ephemeral,
sandboxed container. Downstream code only ever depends on the `ContainerOps`
trait, never on a concrete runtime — so the same orchestration logic works
against Docker, a rootless OCI runtime, or a CLI-based backend.

## What it provides

- **The `ContainerOps` trait** — the full container lifecycle: create, start,
  stop, remove, restart, exec, file copy in/out, filesystem snapshots and
  diffs, volumes, and images.
- **A reference Docker backend** — `ContainerManager` drives the Docker Engine
  HTTP API via [Bollard](https://crates.io/crates/bollard) (the default
  `docker` feature).
- **A rootless OCI backend** — the `cherino-runtime` crate wraps
  [libcontainer](https://github.com/containers/youki) (the library underlying
  the Youki OCI runtime) as a daemonless, rootless backend for Linux hosts
  without Docker.
- **A shared security-profile layer** — seccomp profiles, an AppArmor FUSE
  profile, Landlock rules, network egress policies, and registry whitelisting,
  shared by all backends so every runtime enforces the same policy.

## Crate layout

| Crate | Description |
|-------|-------------|
| `cherino-macros` | `Getters` derive macro used by the DTO types |
| `cherino` | `ContainerOps` trait, Docker backend, security profiles, shared types |
| `cherino-runtime` | Youki/libcontainer OCI backend (Linux-only, non-Linux stubs) |

## Installation

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

Minimal usage against the local Docker daemon:

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

## AppArmor profile

Nested containers (containers created from inside a container) need the FUSE
AppArmor profile installed on the host, as root, once per host:

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Hosts that still carry the legacy `celestia-plana-fuse` profile are detected
and accepted with a deprecation warning; install the new name when you can.
