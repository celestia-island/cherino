<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/master/res/logo/cherino.webp" alt="Cherino" width="240" /></p>

<h1 align="center">Cherino</h1>

<p align="center"><strong>celestia 平台统一的、运行时无关的容器操作工具包</strong></p>

<div align="center">

[![License: SySL-1.0](https://img.shields.io/badge/License-SySL--1.0-blue.svg)](https://sysl.celestia.world)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fcherino-blue.svg)](https://github.com/celestia-island/cherino)
[![Docs](https://img.shields.io/badge/docs-cherino.docs.celestia.world-blue)](https://cherino.docs.celestia.world)
[![docs.rs](https://docs.rs/cherino/badge.svg)](https://docs.rs/cherino)
[![Checks](https://img.shields.io/github/actions/workflow/status/celestia-island/cherino/checks.yml)](https://github.com/celestia-island/cherino/actions/workflows/checks.yml)

</div>

<div align="center">

[English](../../README.md) ·
**简体中文** ·
[繁體中文](../zh-Hant/README.md) ·
[日本語](../ja/README.md) ·
[한국어](../ko/README.md) ·
[Français](../fr/README.md) ·
[Español](../es/README.md) ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

Cherino 是 celestia 平台的容器操作工具包——一个独立的
[Rust](https://www.rust-lang.org/) 库，为创建和管理沙箱容器提供统一的、
运行时无关的 API。

`cherino` 定义了 [`ContainerOps`] trait——完整的容器生命周期
（创建、启动、停止、exec、文件复制、快照、卷、镜像）——外加一个参考性的
Docker 实现和一个共享的安全配置层（seccomp、AppArmor、Landlock、egress
控制、registry 白名单）。`cherino-runtime` 则增加了一个基于
[libcontainer](https://github.com/containers/youki) 的 OCI 原生 rootless
backend。

## Crate 布局

| Crate | 描述 |
|-------|-------------|
| [`cherino-macros`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-macros) | DTO 类型使用的 `Getters` derive 宏 |
| [`cherino`](https://github.com/celestia-island/cherino/tree/master/crates/cherino) | `ContainerOps` trait、Docker backend、安全配置、共享类型 |
| [`cherino-runtime`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-runtime) | Youki/libcontainer OCI backend（仅限 Linux，非 Linux 平台为 stub） |

## ContainerOps backends

| Backend | 类型 | 平台 | 机制 | 层级 |
|---------|------|----------|-----------|------|
| **Docker** | API | 全部 | Bollard → Docker Engine HTTP API | primary |
| **Youki** | Native | Linux | libcontainer → OCI rootless 容器 | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell 调用 | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI（每容器一个 VM） | fallback |

**Youki 属于 fallback 层级**：`cherino-runtime` 中的 libcontainer backend
是面向没有 Docker 的主机的 rootless、无守护进程替代方案，而不是主要的
编排驱动。Docker backend 是默认路径，也是经过最多实战检验的路径。

## Features

`cherino`：

- `docker` *（默认）*——基于 Bollard 的 `ContainerManager` backend。
- `cli-backend`——WSLc / Apple Container CLI 适配器（`cli-backend`
  模块）。默认关闭；在 Windows/macOS 主机上需显式启用。
- `docker-tests`——需要存活 Docker 守护进程的集成测试
  （默认关闭；隐含 `docker`）。

`cherino-runtime` 在 `cfg(target_os = "linux")` 下自动构建其 Linux
backend；所有其他平台编译为返回"only available on Linux"错误的 stub。

## 快速上手

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

加载嵌套容器所需的 FUSE AppArmor profile（主机上、以 root 身份、每台主机一次）：

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

仍携带旧版 `celestia-plana-fuse` profile 的主机会被检测到并以弃用警告
接受；请在方便时安装新名称的 profile。

## 品牌与兼容性

`cherino` 是从 `plana` workspace 中拆分出来的。以下旧名称仍会被读取以
保持兼容，每次读取都会发出一条 `tracing::warn!`：

| 旧名称（plana / entelecheia） | 新名称（cherino） |
|------------------------------|---------------|
| AppArmor profile `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` 环境变量 | `CHERINO_APPARMOR_UNCONFINED` 环境变量 |
| `ENTELECHEIA_RUN_DIR` 环境变量 | `CHERINO_RUN_DIR` 环境变量 |
| `/tmp/entelecheia/youki` 运行目录 | `/tmp/cherino/youki` 运行目录 |

通用覆盖项（`CONTAINER_RUN_DIR`、`CONTAINER_ROOTFS_URL`、
`CONTAINER_NETWORK`）的优先级高于所有带品牌名称的变量。

## AI 生成披露

本仓库的代码主要由 AI 生成，并以 [SySL-1.0](../../LICENSE) 许可证发布。
许可证文本、附加到本仓库 [LICENSE](../../LICENSE) 的模型披露以及 FAQ，
请参见 `sysl` 仓库（<https://github.com/celestia-island/sysl>）。

## 许可证

以 [SySL-1.0](../../LICENSE) 许可证发布。使用本软件即表示您接受其
AI 生成披露与风险确认条款。
