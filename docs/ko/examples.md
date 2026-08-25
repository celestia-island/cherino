# 예제

모든 예제는 실제 `cherino` / `cherino-runtime` API를 대상으로
컴파일됩니다. 먼저 crate를 추가하세요:

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## 컨테이너 목록 조회 (Docker 백엔드)

최소한의 빠른 시작: 로컬 Docker 데몬에 연결하고 `ContainerOps` trait을
통해 실행 중인 컨테이너를 나열합니다.

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

## 생성, 시작, exec, 제거

`ContainerOps`를 통한 전체 생명주기입니다. `ContainerCreateParams::simple`은
모든 필드를 안전한 기본값으로 채우므로, 필요한 것만 재정의하세요.

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

## Rootless OCI 컨테이너 (Youki 백엔드)

`cherino-runtime`의 `YoukiManager`는 Linux에서 동일한 `ContainerOps`
trait을 rootless이자 daemonless로 구현합니다. 동일한 호출 시퀀스가
그대로 동작하며 — 생성 방식만 다릅니다.

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

비 Linux 플랫폼에서 `cherino-runtime`은 동일한 타입과 trait 구현을 가진
스텁을 컴파일하며, 호출 시 "only available on Linux" 오류를 반환하므로
크로스 플랫폼 코드가 수정 없이 컴파일됩니다.

## 보안 프로필 적용

플랫폼 워크로드는 `cherino::security_profile`의 미리 만들어진 프로필을
사용합니다. 각 `ContainerSecurity`는 `ContainerCreateParams` 필드와
일대일로 매핑됩니다:

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

## 사용자 정의 egress 정책

egress 화이트리스트를 플루언트하게 구성하고 컨테이너 파라미터에
연결합니다:

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## Snowflake 매니저에 관한 참고 사항

Snowflake 매니저 — 에이전트 워크로드를 실행하는 워크스페이스별 샌드박스
오케스트레이터 — 는 **cherino의 일부가 아닙니다**: 내부(Cosmos) 런타임
계층의 소비자로서 entelecheia 워크스페이스 다운스트림에 존재합니다.
`ContainerOps`를 통해 백엔드를 선택하며(기본값은 Youki/libcontainer,
`COSMOS_CONTAINER_RUNTIME=youki`), `cherino::security_profile::cosmos()`
위에 자체 워크스페이스별 egress 정책을 얹습니다. 따라야 할 패턴은 위와
같습니다: `ContainerOps` trait에 의존하고, 호스트가 지원하는 백엔드를
생성하며, 공유 보안 프로필을 적용하는 것입니다.
