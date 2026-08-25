# バックエンド

Cherino はコンテナのサンドボックス化を「機構の上にポリシー
(policy-over-mechanism)」の問題として扱います。プラットフォームは
(セキュリティプロファイルを通じて) *何が* 許可されるかを規定し、
各バックエンドがそれを自前の機構 (Docker ホスト設定や OCI spec
フック) に変換します。すべてのバックエンドは同じ `ContainerOps`
トレイトを実装します。

| バックエンド | 種別 | プラットフォーム | 機構 | ティア |
|---------|------|----------|-----------|------|
| **Docker** | API | すべて | Bollard → Docker Engine HTTP API | primary |
| **Youki** | Native | Linux | libcontainer → OCI rootless コンテナ | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` のシェル呼び出し | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI (コンテナごとの VM) | fallback |

## Docker (primary)

Docker バックエンドはデフォルトであり、最も実績のある経路です。
`ContainerManager` は Bollard を通じてローカルの Docker デーモンに
接続し、exec、ファイルコピー、スナップショット、ボリューム、
イメージ管理を含む `ContainerOps` の全インターフェースを実装します。
デフォルトの `docker` feature の下で、すべてのプラットフォームで
利用できます。

```rust
use cherino::ContainerManager;

let mgr = ContainerManager::new()?; // local daemon
// or, against a specific socket:
let mgr = ContainerManager::new_with_socket("/var/run/docker.sock")?;
```

## Youki / libcontainer (fallback, Linux)

`cherino-runtime` crate は、libcontainer 上に構築された OCI ネイティブの
バックエンド `YoukiManager` を提供します。これは **rootless かつ
daemonless** です: コンテナは呼び出したユーザーとして実行され、
バックグラウンドのサービスを必要としません。そのため、Docker デーモンが
利用できない、あるいは許可されていないホストでのフォールバックに
なります。

`cherino-runtime` は `cfg(target_os = "linux")` で実際のバックエンドを
自動的にビルドします。その他すべてのプラットフォームでは、呼び出しが
「only available on Linux」エラーを返すスタブがコンパイルされるため、
クロスプラットフォームのコードは無条件に依存できます。

**Youki は fallback ティアです**: プライマリのオーケストレーション
ドライバーではなく、rootless な代替手段として保守されています。
デーモンが利用可能なときは常に Docker バックエンドを優先してください。

## CLI バックエンド (fallback, Windows / macOS)

オプトインの `cli-backend` feature の下で、cherino は安定したローカル
API を公開しないコンテナランタイム向けの CLI アダプターを同梱します:

- **WSLc** (Windows) — `wslc.exe` / `container.exe` へのシェル呼び出し。
- **Apple Container** (macOS 26+) — `container` CLI を駆動します。
  コンテナごとに軽量な VM を実行します。

`cli-backend` はデフォルトではオフです。Windows/macOS ホストでは
明示的に有効化してください:

```toml
[dependencies]
cherino = { version = "0.1", features = ["cli-backend"] }
```

## 二層のランタイムアーキテクチャ

celestia プラットフォームは、異なるレイヤーで 2 つの異なるコンテナ
ランタイムを使用します:

| レイヤー | ランタイム | 使用元 |
|-------|---------|---------|
| **外側** (オーケストレーション) | Docker/Podman | TUI ヘルスチェック、scepter デーモン、サーバーマネージャー |
| **内側** (cosmos サンドボックス) | Youki/libcontainer | Snowflake マネージャー、Neikos エージェント状態 |

外側のレイヤーは Docker/Podman API 経由でインフラコンテナ (scepter、
postgres) を管理します。これらはネットワーキング、永続ボリューム、
ヘルスチェック、複数コンテナの構成といった完全なオーケストレーション
機能を必要とします。内側のレイヤー (Cosmos) は scepter コンテナの
*内部* で実行され、Youki を使ってエージェント実行用の軽量で起動の速い
サンドボックスコンテナを作成します。それぞれが独自の seccomp
プロファイルとリソース制限を持ちます。

## Feature フラグ

`cherino`:

- `docker` *(デフォルト)* — Bollard ベースの `ContainerManager` バックエンド。
- `cli-backend` — WSLc / Apple Container の CLI アダプター。
  デフォルトではオフ。
- `docker-tests` — 実働中の Docker デーモンを必要とする統合テスト
  (デフォルトではオフ。`docker` を内包)。
