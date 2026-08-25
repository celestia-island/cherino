# Backends

Cherino 將容器沙箱化視為「政策凌駕於機制」的問題：平台規定*允許什麼*
（透過安全性設定檔），而每個 backend 再將其轉譯成各自的原生機制
（Docker host config 或 OCI spec hook）。所有 backend 都實作相同的
`ContainerOps` trait。

| Backend | 類型 | 平台 | 機制 | 層級 |
|---------|------|----------|-----------|------|
| **Docker** | API | 全部 | Bollard → Docker Engine HTTP API | primary |
| **Youki** | Native | Linux | libcontainer → OCI rootless 容器 | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell-out | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI（每個容器一個 VM） | fallback |

## Docker（primary）

Docker backend 是預設值，也是經過最多實戰驗證的路徑。
`ContainerManager` 透過 Bollard 連線到本機 Docker daemon，
並實作完整的 `ContainerOps` 介面，包括 exec、檔案複製、快照、
volume 以及映像管理。它在預設的 `docker` feature 之下，
於每個平台皆可使用。

```rust
use cherino::ContainerManager;

let mgr = ContainerManager::new()?; // local daemon
// or, against a specific socket:
let mgr = ContainerManager::new_with_socket("/var/run/docker.sock")?;
```

## Youki / libcontainer（fallback，Linux）

`cherino-runtime` crate 提供 `YoukiManager`，一個以 libcontainer
為基礎的 OCI 原生 backend。它是 **rootless 且無 daemon 的**：
容器以呼叫者的使用者身分執行，不需要背景服務，因此成為
無法使用或不允許 Docker daemon 的主機的 fallback 選項。

`cherino-runtime` 在 `cfg(target_os = "linux")` 下會自動建構
其真正的 backend。在其他所有平台上，它會編譯成一個 stub，
其呼叫會回傳「only available on Linux」錯誤，因此跨平台程式碼
可以無條件地依賴它。

**Youki 屬於 fallback 層級**：它是作為 rootless 替代方案維護的，
而非主要的編排驅動器。只要有 daemon 可用，請優先使用 Docker
backend。

## CLI backends（fallback，Windows / macOS）

在需明確啟用的 `cli-backend` feature 之下，cherino 為沒有
穩定本機 API 的容器 runtime 提供了 CLI 轉接器：

- **WSLc**（Windows）— shell 呼叫 `wslc.exe` / `container.exe`。
- **Apple Container**（macOS 26+）— 驅動 `container` CLI，
  每個容器運行一個輕量級 VM。

`cli-backend` 預設關閉；在 Windows/macOS 主機上請明確啟用：

```toml
[dependencies]
cherino = { version = "0.1", features = ["cli-backend"] }
```

## 雙層 runtime 架構

celestia 平台在不同層級使用兩種不同的容器 runtime：

| 層級 | Runtime | 使用者 |
|-------|---------|---------|
| **外層**（編排） | Docker/Podman | TUI 健康檢查、scepter daemon、server manager |
| **內層**（cosmos 沙箱） | Youki/libcontainer | Snowflake manager、Neikos agent state |

外層透過 Docker/Podman API 管理基礎設施容器（scepter、postgres）；
這些容器需要完整的編排功能——網路、持久化 volume、健康檢查、
多容器組合。內層（Cosmos）運行在 scepter 容器*內部*，使用 Youki
建立輕量、快速啟動的沙箱容器供 agent 執行，每個容器都有自己的
seccomp profile 與資源限制。

## Feature flags

`cherino`：

- `docker` *（預設）* — 以 Bollard 為基礎的 `ContainerManager` backend。
- `cli-backend` — WSLc / Apple Container 的 CLI 轉接器。預設關閉。
- `docker-tests` — 需要運行中 Docker daemon 的整合測試
  （預設關閉；隱含 `docker`）。
