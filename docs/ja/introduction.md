# はじめに

Cherino は celestia プラットフォームのコンテナ操作ツールキットです。
サンドボックス化されたコンテナの作成と管理のための、統一された
ランタイム非依存 API を提供する、スタンドアロンの Rust ライブラリです。

celestia プラットフォーム上のすべてのサンドボックス化されたワークロードは、
エフェメラルなサンドボックスコンテナの内部で実行されます。下流のコードは
常に `ContainerOps` トレイトにのみ依存し、具体的なランタイムには
依存しません。そのため、同じオーケストレーションロジックが Docker、
rootless OCI ランタイム、CLI ベースのバックエンドのいずれに対しても
動作します。

## 提供する機能

- **`ContainerOps` トレイト** — コンテナのフルライフサイクル: 作成、起動、
  停止、削除、再起動、exec、ファイルのコピーイン/コピーアウト、
  ファイルシステムのスナップショットと差分、ボリューム、イメージ。
- **リファレンス Docker バックエンド** — `ContainerManager` は
  [Bollard](https://crates.io/crates/bollard) 経由で Docker Engine
  HTTP API を駆動します (デフォルトの `docker` feature)。
- **rootless OCI バックエンド** — `cherino-runtime` crate は
  [libcontainer](https://github.com/containers/youki) (Youki OCI
  ランタイムの基盤となるライブラリ) をラップし、Docker のない Linux
  ホスト向けの daemonless・rootless バックエンドを提供します。
- **共通セキュリティプロファイルレイヤー** — seccomp プロファイル、
  AppArmor FUSE プロファイル、Landlock ルール、ネットワーク egress
  ポリシー、レジストリホワイトリスト。すべてのバックエンドで共有され、
  どのランタイムも同一のポリシーを強制します。

## Crate 構成

| Crate | 説明 |
|-------|-------------|
| `cherino-macros` | DTO 型で使用される `Getters` derive マクロ |
| `cherino` | `ContainerOps` トレイト、Docker バックエンド、セキュリティプロファイル、共有型 |
| `cherino-runtime` | Youki/libcontainer OCI バックエンド (Linux 専用、非 Linux ではスタブ) |

## インストール

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

ローカルの Docker デーモンに対する最小限の使用例:

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

## AppArmor プロファイル

ネストされたコンテナ (コンテナの内部から作成されるコンテナ) には、
FUSE AppArmor プロファイルをホストにインストールする必要があります。
root として、ホストごとに 1 回実行してください:

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

レガシーの `celestia-plana-fuse` プロファイルを残したままのホストは
検出され、非推奨警告付きで受け入れられます。可能なときに新しい名前で
インストールしてください。
