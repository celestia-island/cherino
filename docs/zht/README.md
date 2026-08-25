<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/master/res/logo/cherino.webp" alt="Cherino" width="240" /></p>

<h1 align="center">Cherino</h1>

<p align="center"><strong>celestia 平台專用、統一且與 runtime 無關的容器操作工具組</strong></p>

<div align="center">

[![License: SySL-1.0](https://img.shields.io/badge/License-SySL--1.0-blue.svg)](https://sysl.celestia.world)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fcherino-blue.svg)](https://github.com/celestia-island/cherino)
[![Docs](https://img.shields.io/badge/docs-cherino.docs.celestia.world-blue)](https://cherino.docs.celestia.world)
[![docs.rs](https://docs.rs/cherino/badge.svg)](https://docs.rs/cherino)
[![Checks](https://img.shields.io/github/actions/workflow/status/celestia-island/cherino/checks.yml)](https://github.com/celestia-island/cherino/actions/workflows/checks.yml)

</div>

<div align="center">

[English](../../README.md) ·
[简体中文](../zhs/README.md) ·
**繁體中文** ·
[日本語](../ja/README.md) ·
[한국어](../ko/README.md) ·
[Français](../fr/README.md) ·
[Español](../es/README.md) ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

Cherino 是 celestia 平台的容器操作工具組——一個獨立的
[Rust](https://www.rust-lang.org/) 函式庫，提供統一、與 runtime 無關的
API，用於建立與管理沙箱容器。

`cherino` 定義了 [`ContainerOps`] trait——涵蓋完整的容器生命週期
（建立、啟動、停止、exec、檔案複製、快照、volume、映像）——並附帶一個
Docker 參考實作，以及共用的安全性設定檔層（seccomp、AppArmor、
Landlock、egress 控管、registry 白名單）。`cherino-runtime`
則提供以 [libcontainer](https://github.com/containers/youki) 為基礎的
OCI 原生 rootless backend。

## Crate 配置

| Crate | 說明 |
|-------|-------------|
| [`cherino-macros`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-macros) | DTO 型別使用的 `Getters` derive 巨集 |
| [`cherino`](https://github.com/celestia-island/cherino/tree/master/crates/cherino) | `ContainerOps` trait、Docker backend、安全性設定檔、共用型別 |
| [`cherino-runtime`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-runtime) | Youki/libcontainer OCI backend（僅限 Linux，非 Linux 平台提供 stub） |

## ContainerOps backends

| Backend | 類型 | 平台 | 機制 | 層級 |
|---------|------|----------|-----------|------|
| **Docker** | API | 全部 | Bollard → Docker Engine HTTP API | primary |
| **Youki** | Native | Linux | libcontainer → OCI rootless 容器 | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell-out | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI（每個容器一個 VM） | fallback |

**Youki 屬於 fallback 層級**：`cherino-runtime` 中的 libcontainer backend
是為沒有 Docker 的主機所維護的 rootless、無 daemon 替代方案，
而非主要的編排驅動器。Docker backend 是預設值，也是經過最多實戰驗證的路徑。

## Features

`cherino`：

- `docker` *（預設）* — 以 Bollard 為基礎的 `ContainerManager` backend。
- `cli-backend` — WSLc / Apple Container 的 CLI 轉接器
  （`cli-backend` 模組）。預設關閉；在 Windows/macOS 主機上需明確啟用。
- `docker-tests` — 需要運行中 Docker daemon 的整合測試
  （預設關閉；隱含 `docker`）。

`cherino-runtime` 在 `cfg(target_os = "linux")` 下會自動建構其 Linux
backend；其他所有平台則編譯成會回傳「only available on Linux」錯誤的 stub。

## 快速入門

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

載入巢狀容器所需的 FUSE AppArmor profile（在主機上、以 root 身分、每台主機一次）：

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

仍帶有舊版 `celestia-plana-fuse` profile 的主機會被偵測到，
並在發出棄用警告的情況下被接受；請在方便時安裝新名稱。

## 品牌命名與相容性

`cherino` 是從 `plana` workspace 抽取出來的。為了相容性，
系統仍會讀取以下舊名稱，每次使用都會發出一則 `tracing::warn!`：

| 舊名稱（plana / entelecheia） | 新名稱（cherino） |
|------------------------------|---------------|
| AppArmor profile `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` 環境變數 | `CHERINO_APPARMOR_UNCONFINED` 環境變數 |
| `ENTELECHEIA_RUN_DIR` 環境變數 | `CHERINO_RUN_DIR` 環境變數 |
| `/tmp/entelecheia/youki` 執行目錄 | `/tmp/cherino/youki` 執行目錄 |

通用覆寫項（`CONTAINER_RUN_DIR`、`CONTAINER_ROOTFS_URL`、
`CONTAINER_NETWORK`）的優先順序高於所有品牌名稱。

## AI 生成揭露

本儲存庫的程式碼大部分由 AI 生成，並以
[SySL-1.0](../../LICENSE) 授權釋出。授權條款全文、
附加於本儲存庫 [LICENSE](../../LICENSE) 的模型揭露，以及常見問答，
請參閱 `sysl` 儲存庫
（<https://github.com/celestia-island/sysl>）。

## 授權

以 [SySL-1.0](../../LICENSE) 授權釋出。使用本軟體即表示
您接受其 AI 生成揭露與風險確認條款。
