# 简介

Cherino 是 celestia 平台的容器操作工具包：一个独立的 Rust 库，为创建和
管理沙箱容器提供统一的、运行时无关的 API。

celestia 平台中的每个沙箱工作负载都运行在临时的沙箱容器内。下游代码只
依赖 `ContainerOps` trait，从不依赖具体的运行时——因此同一套编排逻辑可
以对接 Docker、rootless OCI 运行时或基于 CLI 的 backend。

## 提供的能力

- **`ContainerOps` trait**——完整的容器生命周期：创建、启动、停止、
  移除、重启、exec、文件复制进/出、文件系统快照与 diff、卷和镜像。
- **参考性的 Docker backend**——`ContainerManager` 通过
  [Bollard](https://crates.io/crates/bollard) 驱动 Docker Engine HTTP API
  （默认的 `docker` feature）。
- **rootless OCI backend**——`cherino-runtime` crate 将
  [libcontainer](https://github.com/containers/youki)（Youki OCI 运行时
  底层的库）封装为面向无 Docker 的 Linux 主机的无守护进程、rootless
  backend。
- **共享的安全配置层**——seccomp profile、AppArmor FUSE profile、
  Landlock 规则、网络 egress 策略和 registry 白名单，由所有 backend 共享，
  使每个运行时强制执行相同的策略。

## Crate 布局

| Crate | 描述 |
|-------|-------------|
| `cherino-macros` | DTO 类型使用的 `Getters` derive 宏 |
| `cherino` | `ContainerOps` trait、Docker backend、安全配置、共享类型 |
| `cherino-runtime` | Youki/libcontainer OCI backend（仅限 Linux，非 Linux 平台为 stub） |

## 安装

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

对接本地 Docker 守护进程的最小用法：

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

嵌套容器（从容器内部创建的容器）需要在主机上安装 FUSE AppArmor
profile，以 root 身份执行，每台主机一次：

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

仍携带旧版 `celestia-plana-fuse` profile 的主机会被检测到并以弃用警告
接受；请在方便时安装新名称的 profile。
