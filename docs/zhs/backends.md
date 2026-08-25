# Backends

Cherino 将容器沙箱视为一个*策略高于机制*的问题：平台规定*允许做什么*
（通过安全配置），而每个 backend 将其翻译为自己的原生机制（Docker host
配置或 OCI spec hook）。所有 backend 都实现同一个 `ContainerOps` trait。

| Backend | 类型 | 平台 | 机制 | 层级 |
|---------|------|----------|-----------|------|
| **Docker** | API | 全部 | Bollard → Docker Engine HTTP API | primary |
| **Youki** | Native | Linux | libcontainer → OCI rootless 容器 | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell 调用 | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI（每容器一个 VM） | fallback |

## Docker（primary）

Docker backend 是默认路径，也是经过最多实战检验的路径。
`ContainerManager` 通过 Bollard 连接本地 Docker 守护进程，并实现完整的
`ContainerOps` 接口，包括 exec、文件复制、快照、卷和镜像管理。它在默认的
`docker` feature 下可在所有平台使用。

```rust
use cherino::ContainerManager;

let mgr = ContainerManager::new()?; // local daemon
// or, against a specific socket:
let mgr = ContainerManager::new_with_socket("/var/run/docker.sock")?;
```

## Youki / libcontainer（fallback，Linux）

`cherino-runtime` crate 提供 `YoukiManager`，一个构建于 libcontainer 之上
的 OCI 原生 backend。它是 **rootless 且无守护进程的**：容器以调用用户的
身份运行，没有后台服务，这使它成为无法使用或不允许使用 Docker 守护进程
的主机上的 fallback 方案。

`cherino-runtime` 在 `cfg(target_os = "linux")` 下自动构建其真实的
backend。在所有其他平台上，它编译为一个 stub，其调用返回"only
available on Linux"错误，因此跨平台代码可以无条件地依赖它。

**Youki 属于 fallback 层级**：它作为 rootless 替代方案维护，而不是主要的
编排驱动。只要有可用的守护进程，请优先使用 Docker backend。

## CLI backends（fallback，Windows / macOS）

在可选的 `cli-backend` feature 之下，cherino 为不提供稳定本地 API 的
容器运行时提供了 CLI 适配器：

- **WSLc**（Windows）——shell 调用 `wslc.exe` / `container.exe`。
- **Apple Container**（macOS 26+）——驱动 `container` CLI，它为每个容器
  运行一个轻量级 VM。

`cli-backend` 默认关闭；在 Windows/macOS 主机上需显式启用：

```toml
[dependencies]
cherino = { version = "0.1", features = ["cli-backend"] }
```

## 两层运行时架构

celestia 平台在不同层使用两种不同的容器运行时：

| 层 | 运行时 | 使用者 |
|-------|---------|---------|
| **外层**（编排） | Docker/Podman | TUI 健康检查、scepter 守护进程、server manager |
| **内层**（cosmos 沙箱） | Youki/libcontainer | Snowflake manager、Neikos agent 状态 |

外层通过 Docker/Podman API 管理基础设施容器（scepter、postgres）；这些
容器需要完整的编排能力——网络、持久卷、健康检查、多容器组合。内层
（Cosmos）运行在 scepter 容器*内部*，使用 Youki 为 agent 执行创建轻量级、
快速启动的沙箱容器，每个容器都有自己的 seccomp profile 和资源限制。

## Feature flags

`cherino`：

- `docker` *（默认）*——基于 Bollard 的 `ContainerManager` backend。
- `cli-backend`——WSLc / Apple Container CLI 适配器。默认关闭。
- `docker-tests`——需要存活 Docker 守护进程的集成测试
  （默认关闭；隐含 `docker`）。
