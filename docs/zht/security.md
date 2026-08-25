# 安全性

Cherino 透過共用的安全性設定檔層，為沙箱容器實施縱深防禦。
相同的政策型別被每個 backend 使用，因此 Docker 與 Youki
runtime 強制執行完全相同的限制。

## 安全性設定檔

`ContainerSecurity` 打包了每個工作負載的政策：

- `cap_drop` / `cap_add` — Linux capability 集合；
- `security_opt` — Docker 安全性選項（例如 `no-new-privileges:true`）；
- `egress_policy` — 網路 egress 政策（見下文）。

現成的設定檔位於 `cherino::security_profile`：`postgres()`、
`scepter()`、`scepter_readonly()` 以及 `cosmos()`。它們編碼了
平台的強化預設值——例如 `scepter()` 會捨棄 `ALL` capabilities，
只加回其子容器所需的最小集合。

在 `docker` feature 下，`cherino::apply_to_host_config` 會將
`ContainerSecurity` 套用至 Bollard `HostConfig`，把政策轉譯成
Docker 原生設定（包括 egress 規則）。

## seccomp

`SeccompProfile` / `SeccompProfileData` 描述 syscall 過濾
profile，而 `build_security_opts` 會將 profile 渲染成容器設定中的
`security_opt` 項目。seccomp 過濾是第一道防線：它限制沙箱工作負載
究竟能發出哪些 syscall。

## AppArmor

巢狀容器需要掛載 FUSE 檔案系統，而預設的 AppArmor 政策會阻止這點。
Cherino 附帶一個專用 profile，
`celestia-cherino-fuse`（`crates/cherino/src/apparmor/celestia-cherino-fuse`），
需在主機上以 root 身分安裝一次：

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

在執行期間，cherino 會偵測已安裝的 profile
（`installed_profile_name`），並透過 `fuse_security_opts` 附加它。
仍帶有舊版 `celestia-plana-fuse` profile 的主機會在發出棄用警告的
情況下被接受。

逃生艙 `CHERINO_APPARMOR_UNCONFINED`（舊名：
`PLANA_APPARMOR_UNCONFINED`）可為無法安裝任何 profile 的主機
跳過 AppArmor 限制——每次使用都會發出一則 `tracing::warn!`。

## Landlock

`LandlockRules` 表達透過 Linux Landlock 強制執行的檔案系統存取規則，
獨立於容器邊界之外，限制沙箱程序可以觸及哪些路徑。

## Egress 控管

`EgressPolicy` 以三種模式（`EgressMode`）控制對外網路存取：
`DenyAll`（預設）、`AllowAll` 以及 `Whitelist`。政策可以流暢地建構：

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

`EgressPolicy::deny_all()` 封鎖所有 egress；`entelecheia_default()`
回傳平台的標準允許清單。在 Docker 上，政策透過 DNS 鎖定與
`extra_hosts` 項目實現，使得只有白名單中的名稱會解析到真實位址。

## Registry 白名單

`RegistryWhitelist`（搭配 `RegistryEntry`）限制容器可以從哪些
映像 registry 拉取。白名單可以從檔案載入
（`RegistryWhitelist::load`）、從文字解析
（`RegistryWhitelist::parse`），或從 workspace 解析
（`resolve_from_workspace`）。

## Rootless 運作

`cherino-runtime`（Youki/libcontainer）backend 以 rootless、
無 daemon 的方式運行容器：沒有特權 daemon 持有容器狀態，
沙箱工作負載以呼叫者的使用者身分執行——在使用它的主機上
縮小了特權攻擊面。

## 品牌命名與相容性

`cherino` 是從 `plana` workspace 抽取出來的。為了相容性，
系統仍會讀取舊名稱，每次使用都會發出一則 `tracing::warn!`：

| 舊名稱（plana / entelecheia） | 新名稱（cherino） |
|------------------------------|---------------|
| AppArmor profile `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` 環境變數 | `CHERINO_APPARMOR_UNCONFINED` 環境變數 |
| `ENTELECHEIA_RUN_DIR` 環境變數 | `CHERINO_RUN_DIR` 環境變數 |
| `/tmp/entelecheia/youki` 執行目錄 | `/tmp/cherino/youki` 執行目錄 |

通用覆寫項（`CONTAINER_RUN_DIR`、`CONTAINER_ROOTFS_URL`、
`CONTAINER_NETWORK`）的優先順序高於所有品牌名稱。
