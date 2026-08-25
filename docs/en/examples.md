# Examples

All examples compile against the real `cherino` / `cherino-runtime` APIs. Add
the crates first:

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## Listing containers (Docker backend)

The minimal quick start: connect to the local Docker daemon and list running
containers through the `ContainerOps` trait.

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

## Create, start, exec, remove

The full lifecycle through `ContainerOps`. `ContainerCreateParams::simple`
fills every field with a safe default; override what you need.

```rust
use cherino::{ContainerCreateParams, ContainerManager, ops::ContainerOps};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = ContainerManager::new()?;

    let mut params = ContainerCreateParams::simple("cherino-demo", "alpine:latest");
    params.env.insert("GREETING".to_string(), "hello".to_string());

    let info = mgr.create(&params).await?;
    mgr.start(info.id()).await?;

    let out = mgr.exec(info.id(), &["sh", "-c", "echo $GREETING"]).await?;
    println!("exit={:?} stdout={}", out.exit_code, out.stdout.trim());

    mgr.stop(info.id()).await?;
    mgr.remove(info.id(), true).await?;
    Ok(())
}
```

## Rootless OCI containers (Youki backend)

`YoukiManager` from `cherino-runtime` implements the same `ContainerOps`
trait, rootless and daemonless, on Linux. The identical call sequence works —
only the construction differs.

```rust
use std::path::Path;

use cherino::ops::ContainerOps;
use cherino_runtime::YoukiManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = YoukiManager::new(Path::new("/tmp/cherino/youki"))?;
    mgr.initialize().await?;

    let infos = mgr.list().await?;
    println!("{} rootless containers", infos.len());
    Ok(())
}
```

On non-Linux platforms `cherino-runtime` compiles a stub with the same type
and trait implementation whose calls return "only available on Linux" errors,
so cross-platform code compiles unchanged.

## Applying a security profile

Platform workloads use the ready-made profiles from
`cherino::security_profile`. Each `ContainerSecurity` maps one-to-one onto
`ContainerCreateParams` fields:

```rust
use cherino::{ContainerCreateParams, ContainerManager, ops::ContainerOps, security_profile};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = ContainerManager::new()?;

    let sec = security_profile::cosmos();
    let mut params = ContainerCreateParams::simple("cosmos-sandbox", "celestia/cosmos:latest");
    params.cap_drop = sec.cap_drop;
    params.cap_add = sec.cap_add;
    params.security_opt = sec.security_opt;
    params.egress_policy = sec.egress_policy;

    let info = mgr.create(&params).await?;
    println!("created hardened container {}", info.id());
    Ok(())
}
```

## Custom egress policy

Build an egress whitelist fluently and attach it to the container params:

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## A note on the Snowflake manager

The Snowflake manager — the per-workspace sandbox orchestrator that runs
agent workloads — is **not part of cherino**: it lives downstream in the
entelecheia workspace, as a consumer of the inner (Cosmos) runtime layer. It
selects its backend through `ContainerOps`, defaulting to Youki/libcontainer
(`COSMOS_CONTAINER_RUNTIME=youki`), and layers its own per-workspace egress
policy on top of `cherino::security_profile::cosmos()`. The pattern to copy
is the one above: depend on the `ContainerOps` trait, construct whichever
backend the host supports, and apply the shared security profiles.
