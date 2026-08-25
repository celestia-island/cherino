# セキュリティ

Cherino は共通のセキュリティプロファイルレイヤーを通じて、
サンドボックス化されたコンテナに対する多層防御 (defence-in-depth) を
強制します。同一のポリシー型がすべてのバックエンドで消費されるため、
Docker ランタイムと Youki ランタイムは同一の制限を強制します。

## セキュリティプロファイル

`ContainerSecurity` はワークロードごとのポリシーを束ねます:

- `cap_drop` / `cap_add` — Linux capability のセット;
- `security_opt` — Docker セキュリティオプション
  (例: `no-new-privileges:true`);
- `egress_policy` — ネットワーク egress ポリシー (後述)。

既製のプロファイルは `cherino::security_profile` にあります:
`postgres()`、`scepter()`、`scepter_readonly()`、`cosmos()`。
これらはプラットフォームの強化済みデフォルトをエンコードしています。
たとえば、`scepter()` は `ALL` capability をドロップし、そのサブコンテナが
必要とする最小限のセットのみを追加し直します。

`docker` feature では、`cherino::apply_to_host_config` が
`ContainerSecurity` を Bollard の `HostConfig` に適用し、ポリシーを
Docker ネイティブの設定 (egress ルールを含む) に変換します。

## seccomp

`SeccompProfile` / `SeccompProfileData` はシステムコールをフィルタリング
するプロファイルを記述し、`build_security_opts` はプロファイルを
コンテナ設定用の `security_opt` エントリーにレンダリングします。
seccomp フィルタリングは防御の第一線です: サンドボックス化された
ワークロードがそもそもどのシステムコールを発行できるかを制約します。

## AppArmor

ネストされたコンテナは FUSE ファイルシステムをマウントする必要が
ありますが、デフォルトの AppArmor ポリシーはこれをブロックします。
Cherino は専用プロファイル `celestia-cherino-fuse`
(`crates/cherino/src/apparmor/celestia-cherino-fuse`) を同梱しており、
ホストに root として 1 回インストールします:

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

実行時に cherino はインストールされているプロファイルを検出し
(`installed_profile_name`)、`fuse_security_opts` 経由でアタッチします。
レガシーの `celestia-plana-fuse` プロファイルを残したままのホストは、
非推奨警告付きで受け入れられます。

エスケープハッチ `CHERINO_APPARMOR_UNCONFINED` (レガシー:
`PLANA_APPARMOR_UNCONFINED`) は、プロファイルをインストールできない
ホストで AppArmor による制限をスキップします。使用のたびに
`tracing::warn!` が出力されます。

## Landlock

`LandlockRules` は Linux Landlock を通じて強制される
ファイルシステムアクセスルールを表現し、サンドボックス化された
プロセスがコンテナ境界とは独立に、どのパスに触れられるかを
制限します。

## Egress 制御

`EgressPolicy` は 3 つのモード (`EgressMode`) で外向きネットワーク
アクセスを制御します: `DenyAll` (デフォルト)、`AllowAll`、`Whitelist`。
ポリシーは流暢な (fluent) API で構築します:

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

`EgressPolicy::deny_all()` はすべての egress をブロックし、
`entelecheia_default()` はプラットフォームの標準許可リストを返します。
Docker では、このポリシーは DNS ピン留めと `extra_hosts` エントリー
によって実現され、ホワイトリストに登録された名前のみが実アドレスに
名前解決されるようになります。

## レジストリホワイトリスト

`RegistryWhitelist` (`RegistryEntry` とともに) は、コンテナがプル
できるイメージレジストリを制限します。ホワイトリストはファイルから
読み込んだり (`RegistryWhitelist::load`)、テキストからパースしたり
(`RegistryWhitelist::parse`)、ワークスペースから解決したり
(`resolve_from_workspace`) できます。

## Rootless 動作

`cherino-runtime` (Youki/libcontainer) バックエンドは、コンテナを
rootless かつ daemonless で実行します: コンテナの状態を保持する
特権デーモンは存在せず、サンドボックス化されたワークロードは
呼び出したユーザーとして実行されます。これにより、使用されるホスト上での
特権攻撃面が縮小されます。

## ブランディングと互換性

`cherino` は `plana` ワークスペースから抽出されました。レガシー名は
互換性のために引き続き読み取られ、それぞれ `tracing::warn!` を
出力します:

| レガシー (plana / entelecheia) | 新しい名前 (cherino) |
|------------------------------|---------------|
| AppArmor プロファイル `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` 環境変数 | `CHERINO_APPARMOR_UNCONFINED` 環境変数 |
| `ENTELECHEIA_RUN_DIR` 環境変数 | `CHERINO_RUN_DIR` 環境変数 |
| `/tmp/entelecheia/youki` 実行ディレクトリ | `/tmp/cherino/youki` 実行ディレクトリ |

汎用のオーバーライド (`CONTAINER_RUN_DIR`、`CONTAINER_ROOTFS_URL`、
`CONTAINER_NETWORK`) は、すべてのブランド名より優先されます。
