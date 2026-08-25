# 範例

所有範例都針對真實的 `cherino` / `cherino-runtime` API 編譯。
請先加入這些 crate：

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## 列出容器（Docker backend）

最小的快速入門：連線到本機 Docker daemon，並透過 `ContainerOps`
trait 列出運行中的容器。

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

## 建立、啟動、exec、移除

透過 `ContainerOps` 的完整生命週期。`ContainerCreateParams::simple`
會為每個欄位填入安全的預設值；覆寫你需要的部分即可。

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

來自 `cherino-runtime` 的 `YoukiManager` 在 Linux 上以 rootless、
無 daemon 的方式實作相同的 `ContainerOps` trait。完全相同的
呼叫序列可以直接使用——只有建構方式不同。

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

在非 Linux 平台上，`cherino-runtime` 會編譯成一個具有相同型別
與 trait 實作的 stub，其呼叫會回傳「only available on Linux」錯誤，
因此跨平台程式碼可以不加修改地編譯。

## 套用安全性設定檔

平台工作負載使用來自 `cherino::security_profile` 的現成設定檔。
每個 `ContainerSecurity` 都一對一對應到 `ContainerCreateParams`
欄位：

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

## 自訂 egress 政策

以流暢的方式建構 egress 白名單，並將它附加到容器參數：

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## 關於 Snowflake manager 的說明

Snowflake manager——負責運行 agent 工作負載的每個 workspace 的
沙箱編排器——**不屬於 cherino**：它位於下游的 entelecheia
workspace，作為內層（Cosmos）runtime 層的消費者。它透過
`ContainerOps` 選擇自己的 backend，預設為 Youki/libcontainer
（`COSMOS_CONTAINER_RUNTIME=youki`），並在
`cherino::security_profile::cosmos()` 之上疊加自己的每個
workspace 的 egress 政策。值得複製的模式就是上面那一個：
依賴 `ContainerOps` trait、建構主機支援的任何 backend，
並套用共用的安全性設定檔。
