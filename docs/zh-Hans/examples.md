# 示例

所有示例都针对真实的 `cherino` / `cherino-runtime` API 编译。请先添加
crate 依赖：

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## 列出容器（Docker backend）

最小快速上手：连接本地 Docker 守护进程，通过 `ContainerOps` trait 列出
正在运行的容器。

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

## 创建、启动、exec、移除

通过 `ContainerOps` 完成完整生命周期。`ContainerCreateParams::simple`
为每个字段填充安全的默认值；按需覆盖即可。

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

## Rootless OCI 容器（Youki backend）

来自 `cherino-runtime` 的 `YoukiManager` 在 Linux 上以 rootless、无守护
进程的方式实现同一个 `ContainerOps` trait。完全相同的调用序列可以工作
——只有构造方式不同。

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

在非 Linux 平台上，`cherino-runtime` 编译为一个具有相同类型和 trait
实现的 stub，其调用返回"only available on Linux"错误，因此跨平台代码
可以不加修改地编译。

## 应用安全配置

平台工作负载使用 `cherino::security_profile` 中的现成 profile。每个
`ContainerSecurity` 与 `ContainerCreateParams` 的字段一一对应：

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

## 自定义 egress 策略

以流式方式构建 egress 白名单并附加到容器参数：

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## 关于 Snowflake manager 的说明

Snowflake manager——运行 agent 工作负载的每 workspace 沙箱编排器——
**不属于 cherino**：它位于下游的 entelecheia workspace，是内层（Cosmos）
运行时层的消费者。它通过 `ContainerOps` 选择自己的 backend，默认使用
Youki/libcontainer（`COSMOS_CONTAINER_RUNTIME=youki`），并在
`cherino::security_profile::cosmos()` 之上叠加自己的每 workspace egress
策略。值得借鉴的模式就是上面展示的那种：依赖 `ContainerOps` trait，
构造主机支持的任意 backend，并应用共享的安全配置。
