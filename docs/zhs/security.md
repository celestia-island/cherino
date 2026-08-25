# 安全

Cherino 通过共享的安全配置层为沙箱容器实施纵深防御。同一套策略类型被
每个 backend 消费，因此 Docker 和 Youki 运行时强制执行完全相同的限制。

## 安全配置

`ContainerSecurity` 打包了每个工作负载的策略：

- `cap_drop` / `cap_add`——Linux capability 集合；
- `security_opt`——Docker 安全选项（如 `no-new-privileges:true`）；
- `egress_policy`——网络 egress 策略（见下文）。

现成的 profile 位于 `cherino::security_profile`：`postgres()`、
`scepter()`、`scepter_readonly()` 和 `cosmos()`。它们编码了平台的加固
默认值——例如，`scepter()` 丢弃 `ALL` capability，只加回其子容器所需的
最小集合。

在 `docker` feature 下，`cherino::apply_to_host_config` 将
`ContainerSecurity` 应用到 Bollard 的 `HostConfig`，把策略翻译为 Docker
原生设置（包括 egress 规则）。

## seccomp

`SeccompProfile` / `SeccompProfileData` 描述系统调用过滤 profile，
`build_security_opts` 将 profile 渲染为容器配置的 `security_opt` 条目。
Seccomp 过滤是第一道防线：它约束沙箱工作负载究竟可以发起哪些系统调用。

## AppArmor

嵌套容器需要挂载 FUSE 文件系统，而默认的 AppArmor 策略会阻止这一行为。
Cherino 附带了一个专用 profile `celestia-cherino-fuse`
（`crates/cherino/src/apparmor/celestia-cherino-fuse`），需要在主机上以
root 身份安装一次：

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

在运行时，cherino 检测已安装的 profile（`installed_profile_name`）并通过
`fuse_security_opts` 附加它。仍携带旧版 `celestia-plana-fuse` profile 的
主机会以弃用警告被接受。

逃生开关 `CHERINO_APPARMOR_UNCONFINED`（旧名：
`PLANA_APPARMOR_UNCONFINED`）可为无法安装 profile 的主机跳过 AppArmor
限制——每次使用都会发出一条 `tracing::warn!`。

## Landlock

`LandlockRules` 表达通过 Linux Landlock 强制执行的文件系统访问规则，
独立于容器边界限制沙箱进程可以触及的路径。

## Egress 控制

`EgressPolicy` 以三种模式（`EgressMode`）控制出站网络访问：`DenyAll`
（默认）、`AllowAll` 和 `Whitelist`。策略可以流式构建：

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

`EgressPolicy::deny_all()` 阻止所有 egress；`entelecheia_default()` 返回
平台的标准白名单。在 Docker 上，该策略通过 DNS 钉扎和 `extra_hosts`
条目实现，使只有白名单中的名称才能解析到真实地址。

## Registry 白名单

`RegistryWhitelist`（配合 `RegistryEntry`）限制容器可以从哪些镜像
registry 拉取。白名单可以从文件加载（`RegistryWhitelist::load`）、从文本
解析（`RegistryWhitelist::parse`），或从 workspace 解析
（`resolve_from_workspace`）。

## Rootless 运行

`cherino-runtime`（Youki/libcontainer）backend 以 rootless、无守护进程
的方式运行容器：没有特权守护进程持有容器状态，沙箱工作负载以调用用户
的身份执行——在其使用的主机上缩小了特权攻击面。

## 品牌与兼容性

`cherino` 是从 `plana` workspace 中拆分出来的。旧名称仍会被读取以保持
兼容，每次读取都会发出一条 `tracing::warn!`：

| 旧名称（plana / entelecheia） | 新名称（cherino） |
|------------------------------|---------------|
| AppArmor profile `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` 环境变量 | `CHERINO_APPARMOR_UNCONFINED` 环境变量 |
| `ENTELECHEIA_RUN_DIR` 环境变量 | `CHERINO_RUN_DIR` 环境变量 |
| `/tmp/entelecheia/youki` 运行目录 | `/tmp/cherino/youki` 运行目录 |

通用覆盖项（`CONTAINER_RUN_DIR`、`CONTAINER_ROOTFS_URL`、
`CONTAINER_NETWORK`）的优先级高于所有带品牌名称的变量。
