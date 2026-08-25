# 使用例

すべての例は、実際の `cherino` / `cherino-runtime` API に対して
コンパイルされます。まず crate を追加してください:

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## コンテナの一覧表示 (Docker バックエンド)

最小限のクイックスタート: ローカルの Docker デーモンに接続し、
`ContainerOps` トレイトを通じて実行中のコンテナを一覧表示します。

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

## 作成、起動、exec、削除

`ContainerOps` を通じたフルライフサイクルです。
`ContainerCreateParams::simple` はすべてのフィールドを安全な
デフォルト値で埋めます。必要な部分だけ上書きしてください。

```rust
use cherino::{ContainerCreateParams, ContainerManager, ops::ContainerOps};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = ContainerManager::new()?;

    let mut params = ContainerCreateParams::simple("cherino-demo", "alpine:latest");
    params.env.insert("GREETING".to_string(), "hello".to_string());

    let info = mgr.create(&params).await?;
    mgr.start(info.id()).await?;

    let out = mgr.exec(info.id(), &["sh", "-c", "echo $GREETING"]).await?;
    println!("exit={:?} stdout={}", out.exit_code, out.stdout.trim());

    mgr.stop(info.id()).await?;
    mgr.remove(info.id(), true).await?;
    Ok(())
}
```

## Rootless OCI コンテナ (Youki バックエンド)

`cherino-runtime` の `YoukiManager` は、同じ `ContainerOps` トレイトを
Linux 上で rootless かつ daemonless に実装します。まったく同じ呼び出し
シーケンスが動作します — 異なるのは構築方法だけです。

```rust
use std::path::Path;

use cherino::ops::ContainerOps;
use cherino_runtime::YoukiManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = YoukiManager::new(Path::new("/tmp/cherino/youki"))?;
    mgr.initialize().await?;

    let infos = mgr.list().await?;
    println!("{} rootless containers", infos.len());
    Ok(())
}
```

非 Linux プラットフォームでは、`cherino-runtime` は同じ型とトレイト
実装を持ち、呼び出しが「only available on Linux」エラーを返すスタブを
コンパイルするため、クロスプラットフォームのコードは変更なしで
コンパイルできます。

## セキュリティプロファイルの適用

プラットフォームのワークロードは `cherino::security_profile` の
既製プロファイルを使用します。各 `ContainerSecurity` は
`ContainerCreateParams` のフィールドに 1 対 1 で対応します:

```rust
use cherino::{ContainerCreateParams, ContainerManager, ops::ContainerOps, security_profile};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = ContainerManager::new()?;

    let sec = security_profile::cosmos();
    let mut params = ContainerCreateParams::simple("cosmos-sandbox", "celestia/cosmos:latest");
    params.cap_drop = sec.cap_drop;
    params.cap_add = sec.cap_add;
    params.security_opt = sec.security_opt;
    params.egress_policy = sec.egress_policy;

    let info = mgr.create(&params).await?;
    println!("created hardened container {}", info.id());
    Ok(())
}
```

## カスタム egress ポリシー

egress ホワイトリストを流暢な (fluent) API で構築し、コンテナ
パラメータにアタッチします:

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## Snowflake マネージャーに関する補足

Snowflake マネージャー — エージェントワークロードを実行する
ワークスペースごとのサンドボックスオーケストレーター — は **cherino の
一部ではありません**: 内側 (Cosmos) ランタイムレイヤーの消費者として、
下流の entelecheia ワークスペースに存在します。バックエンドは
`ContainerOps` を通じて選択され、デフォルトは Youki/libcontainer
(`COSMOS_CONTAINER_RUNTIME=youki`) で、`cherino::security_profile::cosmos()`
の上に独自のワークスペースごとの egress ポリシーを重ねています。
真似すべきパターンは上記のものです: `ContainerOps` トレイトに依存し、
ホストがサポートするバックエンドを構築し、共有のセキュリティ
プロファイルを適用してください。
