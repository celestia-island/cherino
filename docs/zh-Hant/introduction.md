# 簡介

Cherino 是 celestia 平台的容器操作工具組：一個獨立的 Rust 函式庫，
提供統一、與 runtime 無關的 API，用於建立與管理沙箱容器。

celestia 平台中每一個沙箱工作負載都運行在一個短生命週期的沙箱容器內。
下游程式碼只依賴 `ContainerOps` trait，絕不依賴具體的 runtime——
因此同一套編排邏輯可以對 Docker、rootless OCI runtime
或 CLI 型 backend 運作。

## 提供的功能

- **`ContainerOps` trait** — 完整的容器生命週期：建立、啟動、停止、
  移除、重新啟動、exec、檔案複製進出、檔案系統快照與差異、
  volume 以及映像。
- **Docker 參考 backend** — `ContainerManager` 透過
  [Bollard](https://crates.io/crates/bollard) 驅動 Docker Engine
  HTTP API（預設的 `docker` feature）。
- **rootless OCI backend** — `cherino-runtime` crate 將
  [libcontainer](https://github.com/containers/youki)（Youki OCI
  runtime 底層的函式庫）包裝成無 daemon、rootless 的 backend，
  供沒有 Docker 的 Linux 主機使用。
- **共用的安全性設定檔層** — seccomp profile、AppArmor FUSE
  profile、Landlock 規則、網路 egress 政策，以及 registry 白名單，
  由所有 backend 共用，讓每個 runtime 強制執行相同的政策。

## Crate 配置

| Crate | 說明 |
|-------|-------------|
| `cherino-macros` | DTO 型別使用的 `Getters` derive 巨集 |
| `cherino` | `ContainerOps` trait、Docker backend、安全性設定檔、共用型別 |
| `cherino-runtime` | Youki/libcontainer OCI backend（僅限 Linux，非 Linux 平台提供 stub） |

## 安裝

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

對本機 Docker daemon 的最小用法：

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

巢狀容器（從容器內部建立的容器）需要在主機上安裝 FUSE AppArmor
profile，以 root 身分執行，每台主機一次：

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

仍帶有舊版 `celestia-plana-fuse` profile 的主機會被偵測到，
並在發出棄用警告的情況下被接受；請在方便時安裝新名稱。
