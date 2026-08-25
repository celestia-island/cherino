# Backends

Cherino treats container sandboxing as a *policy-over-mechanism* problem: the
platform prescribes *what* is allowed (via security profiles), and each
backend translates that into its native mechanism (Docker host configs or OCI
spec hooks). All backends implement the same `ContainerOps` trait.

| Backend | Type | Platform | Mechanism | Tier |
|---------|------|----------|-----------|------|
| **Docker** | API | All | Bollard → Docker Engine HTTP API | primary |
| **Youki** | Native | Linux | libcontainer → OCI rootless containers | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell-out | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI (VM-per-container) | fallback |

## Docker (primary)

The Docker backend is the default and the most battle-tested path.
`ContainerManager` connects to the local Docker daemon through Bollard and
implements the full `ContainerOps` surface, including exec, file copy,
snapshots, volumes, and image management. It is available on every platform
behind the default `docker` feature.

```rust
use cherino::ContainerManager;

let mgr = ContainerManager::new()?; // local daemon
// or, against a specific socket:
let mgr = ContainerManager::new_with_socket("/var/run/docker.sock")?;
```

## Youki / libcontainer (fallback, Linux)

The `cherino-runtime` crate provides `YoukiManager`, an OCI-native backend
built on libcontainer. It is **rootless and daemonless**: containers run as
the calling user with no background service, which makes it the fallback for
hosts where no Docker daemon is available or permitted.

`cherino-runtime` builds its real backend automatically on
`cfg(target_os = "linux")`. On every other platform it compiles a stub whose
calls return "only available on Linux" errors, so cross-platform code can
depend on it unconditionally.

**Youki is fallback-tier**: it is maintained as a rootless alternative, not as
the primary orchestration driver. Prefer the Docker backend whenever a daemon
is available.

## CLI backends (fallback, Windows / macOS)

Behind the opt-in `cli-backend` feature, cherino ships CLI adapters for
container runtimes that expose no stable local API:

- **WSLc** on Windows — shells out to `wslc.exe` / `container.exe`.
- **Apple Container** on macOS 26+ — drives the `container` CLI, which runs a
  lightweight VM per container.

`cli-backend` is off by default; enable it explicitly on Windows/macOS hosts:

```toml
[dependencies]
cherino = { version = "0.1", features = ["cli-backend"] }
```

## Two-layer runtime architecture

The celestia platform uses two distinct container runtimes at different
layers:

| Layer | Runtime | Used by |
|-------|---------|---------|
| **Outer** (orchestration) | Docker/Podman | TUI health check, scepter daemon, server manager |
| **Inner** (cosmos sandbox) | Youki/libcontainer | Snowflake manager, Neikos agent state |

The outer layer manages infrastructure containers (scepter, postgres) via the
Docker/Podman API; these need full orchestration features — networking,
persistent volumes, health checks, multi-container composition. The inner
layer (Cosmos) runs *inside* the scepter container and uses Youki to create
lightweight, fast-start sandboxed containers for agent execution, each with
its own seccomp profile and resource limits.

## Feature flags

`cherino`:

- `docker` *(default)* — the Bollard-based `ContainerManager` backend.
- `cli-backend` — the WSLc / Apple Container CLI adapters. Off by default.
- `docker-tests` — integration tests that require a live Docker daemon
  (off by default; implies `docker`).
