<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/master/res/logo/cherino.webp" alt="Cherino" width="240" /></p>

<h1 align="center">Cherino</h1>

<p align="center"><strong>celestia プラットフォーム向けの、統一されたランタイム非依存コンテナ操作ツールキット</strong></p>

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
[繁體中文](../zht/README.md) ·
**日本語** ·
[한국어](../ko/README.md) ·
[Français](../fr/README.md) ·
[Español](../es/README.md) ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

Cherino は celestia プラットフォーム向けのコンテナ操作ツールキットです。
サンドボックス化されたコンテナの作成と管理のための、統一された
ランタイム非依存 API を提供する、スタンドアロンの
[Rust](https://www.rust-lang.org/) ライブラリです。

`cherino` は [`ContainerOps`] トレイト — コンテナのフルライフサイクル
(作成、起動、停止、exec、ファイルコピー、スナップショット、ボリューム、
イメージ) — に加えて、リファレンス実装となる Docker バックエンドと、
共通のセキュリティプロファイルレイヤー (seccomp、AppArmor、Landlock、
egress 制御、レジストリホワイトリスト) を定義します。`cherino-runtime` は
[libcontainer](https://github.com/containers/youki) 上に構築された
OCI ネイティブの rootless バックエンドを追加します。

## Crate 構成

| Crate | 説明 |
|-------|-------------|
| [`cherino-macros`](../../crates/cherino-macros) | DTO 型で使用される `Getters` derive マクロ |
| [`cherino`](../../crates/cherino) | `ContainerOps` トレイト、Docker バックエンド、セキュリティプロファイル、共有型 |
| [`cherino-runtime`](../../crates/cherino-runtime) | Youki/libcontainer OCI バックエンド (Linux 専用、非 Linux ではスタブ) |

## ContainerOps バックエンド

| バックエンド | 種別 | プラットフォーム | 機構 | ティア |
|---------|------|----------|-----------|------|
| **Docker** | API | すべて | Bollard → Docker Engine HTTP API | primary |
| **Youki** | Native | Linux | libcontainer → OCI rootless コンテナ | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` のシェル呼び出し | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI (コンテナごとの VM) | fallback |

**Youki は fallback ティアです**: `cherino-runtime` の libcontainer
バックエンドは、Docker のないホスト向けの rootless・daemonless な
代替手段として保守されており、プライマリのオーケストレーション
ドライバーとしては位置づけられていません。Docker バックエンドが
デフォルトであり、最も実績のある経路です。

## Features

`cherino`:

- `docker` *(デフォルト)* — Bollard ベースの `ContainerManager` バックエンド。
- `cli-backend` — WSLc / Apple Container の CLI アダプター
  (`cli-backend` モジュール)。デフォルトではオフ。Windows/macOS ホストでは
  明示的に有効化してください。
- `docker-tests` — 実働中の Docker デーモンを必要とする統合テスト
  (デフォルトではオフ。`docker` を内包)。

`cherino-runtime` は `cfg(target_os = "linux")` で Linux バックエンドを
自動的にビルドします。その他のプラットフォームでは
「only available on Linux」エラーを返すスタブがコンパイルされます。

## クイックスタート

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

ネストされたコンテナに必要な FUSE AppArmor プロファイルの読み込み
(ホスト上で、root として、ホストごとに 1 回):

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

レガシーの `celestia-plana-fuse` プロファイルを残したままのホストは
検出され、非推奨警告付きで受け入れられます。可能なときに新しい名前で
インストールしてください。

## ブランディングと互換性

`cherino` は `plana` ワークスペースから抽出されました。以下のレガシー名は
互換性のために引き続き読み取られ、それぞれ `tracing::warn!` を出力します。

| レガシー (plana / entelecheia) | 新しい名前 (cherino) |
|------------------------------|---------------|
| AppArmor プロファイル `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` 環境変数 | `CHERINO_APPARMOR_UNCONFINED` 環境変数 |
| `ENTELECHEIA_RUN_DIR` 環境変数 | `CHERINO_RUN_DIR` 環境変数 |
| `/tmp/entelecheia/youki` 実行ディレクトリ | `/tmp/cherino/youki` 実行ディレクトリ |

汎用のオーバーライド (`CONTAINER_RUN_DIR`、`CONTAINER_ROOTFS_URL`、
`CONTAINER_NETWORK`) は、すべてのブランド名より優先されます。

## AI 生成の開示

このリポジトリのコードは実質的に AI によって生成されており、
[SySL-1.0](../../LICENSE) ライセンスの下でライセンスされています。
ライセンス本文、このリポジトリの [LICENSE](../../LICENSE) に付記された
モデル開示、および FAQ については、`sysl` リポジトリ
(<https://github.com/celestia-island/sysl>) を参照してください。

## ライセンス

[SySL-1.0](../../LICENSE) ライセンスの下でライセンスされています。
本ソフトウェアを使用することで、その AI 生成の開示および
リスク承認条項に同意したものとみなされます。
