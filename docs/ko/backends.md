# 백엔드

Cherino는 컨테이너 샌드박싱을 *메커니즘 위의 정책(policy-over-mechanism)*
문제로 취급합니다: 플랫폼은 *무엇이* 허용되는지를 (보안 프로필을 통해)
규정하고, 각 백엔드는 이를 자신의 네이티브 메커니즘(Docker 호스트 설정
또는 OCI spec 훅)으로 변환합니다. 모든 백엔드는 동일한 `ContainerOps`
trait을 구현합니다.

| 백엔드 | 유형 | 플랫폼 | 메커니즘 | 티어 |
|---------|------|----------|-----------|------|
| **Docker** | API | 전체 | Bollard → Docker Engine HTTP API | primary |
| **Youki** | 네이티브 | Linux | libcontainer → OCI rootless 컨테이너 | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` 셸 호출 | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI (컨테이너당 VM) | fallback |

## Docker (primary)

Docker 백엔드는 기본값이며 가장 검증된 경로입니다.
`ContainerManager`는 Bollard를 통해 로컬 Docker 데몬에 연결하여
exec, 파일 복사, 스냅샷, 볼륨, 이미지 관리를 포함한 `ContainerOps`의
전체 인터페이스를 구현합니다. 기본 `docker` feature 뒤에서 모든
플랫폼에서 사용할 수 있습니다.

```rust
use cherino::ContainerManager;

let mgr = ContainerManager::new()?; // local daemon
// or, against a specific socket:
let mgr = ContainerManager::new_with_socket("/var/run/docker.sock")?;
```

## Youki / libcontainer (fallback, Linux)

`cherino-runtime` crate는 libcontainer 위에 구축된 OCI 네이티브 백엔드인
`YoukiManager`를 제공합니다. **rootless이자 daemonless**입니다:
컨테이너는 백그라운드 서비스 없이 호출한 사용자 권한으로 실행되므로,
Docker 데몬을 사용할 수 없거나 허용되지 않는 호스트에서 fallback이
됩니다.

`cherino-runtime`은 `cfg(target_os = "linux")`에서 실제 백엔드를 자동으로
빌드합니다. 다른 모든 플랫폼에서는 호출 시 "only available on Linux"
오류를 반환하는 스텁을 컴파일하므로, 크로스 플랫폼 코드가 무조건 이에
의존할 수 있습니다.

**Youki는 fallback 티어입니다**: 기본 오케스트레이션 드라이버가 아닌
rootless 대안으로 유지 관리됩니다. 데몬을 사용할 수 있는 경우에는
항상 Docker 백엔드를 우선하세요.

## CLI 백엔드 (fallback, Windows / macOS)

opt-in `cli-backend` feature 뒤에서, cherino는 안정적인 로컬 API를
노출하지 않는 컨테이너 런타임용 CLI 어댑터를 제공합니다:

- Windows의 **WSLc** — `wslc.exe` / `container.exe`를 셸로 호출.
- macOS 26+의 **Apple Container** — 컨테이너당 경량 VM을 실행하는
  `container` CLI를 구동.

`cli-backend`는 기본적으로 비활성화되어 있습니다; Windows/macOS
호스트에서 명시적으로 활성화하세요:

```toml
[dependencies]
cherino = { version = "0.1", features = ["cli-backend"] }
```

## 2계층 런타임 아키텍처

celestia 플랫폼은 서로 다른 계층에서 두 가지 컨테이너 런타임을
사용합니다:

| 계층 | 런타임 | 사용처 |
|-------|---------|---------|
| **외부** (오케스트레이션) | Docker/Podman | TUI 헬스 체크, scepter 데몬, 서버 매니저 |
| **내부** (cosmos 샌드박스) | Youki/libcontainer | Snowflake 매니저, Neikos 에이전트 상태 |

외부 계층은 Docker/Podman API를 통해 인프라 컨테이너(scepter,
postgres)를 관리합니다; 이들은 네트워킹, 영구 볼륨, 헬스 체크,
다중 컨테이너 구성 등 완전한 오케스트레이션 기능이 필요합니다.
내부 계층(Cosmos)은 scepter 컨테이너 *내부에서* 실행되며 Youki를
사용해 에이전트 실행을 위한 가볍고 빠르게 시작되는 샌드박스 컨테이너를
생성하며, 각각 자체 seccomp 프로필과 리소스 제한을 가집니다.

## Feature 플래그

`cherino`:

- `docker` *(기본값)* — Bollard 기반 `ContainerManager` 백엔드.
- `cli-backend` — WSLc / Apple Container CLI 어댑터. 기본적으로 비활성화.
- `docker-tests` — 실행 중인 Docker 데몬이 필요한 통합 테스트
  (기본적으로 비활성화; `docker`를 포함).
