# 소개

Cherino는 celestia 플랫폼의 컨테이너 운영 툴킷입니다. 샌드박스화된
컨테이너를 생성하고 관리하기 위한 통합적이고 런타임에 구애받지 않는
API를 제공하는 독립형 Rust 라이브러리입니다.

celestia 플랫폼의 모든 샌드박스 워크로드는 일시적이고 샌드박스화된
컨테이너 안에서 실행됩니다. 다운스트림 코드는 항상 `ContainerOps`
trait에만 의존하며 구체적인 런타임에는 의존하지 않습니다 — 따라서 동일한
오케스트레이션 로직이 Docker, rootless OCI 런타임, CLI 기반 백엔드에서
모두 동작합니다.

## 제공하는 기능

- **`ContainerOps` trait** — 컨테이너의 전체 생명주기: 생성, 시작,
  중지, 제거, 재시작, exec, 파일 복사(입출력), 파일시스템 스냅샷과
  diff, 볼륨, 이미지.
- **레퍼런스 Docker 백엔드** — `ContainerManager`는
  [Bollard](https://crates.io/crates/bollard)를 통해 Docker Engine
  HTTP API를 구동합니다(기본 `docker` feature).
- **rootless OCI 백엔드** — `cherino-runtime` crate는
  [libcontainer](https://github.com/containers/youki)(Youki OCI 런타임의
  기반이 되는 라이브러리)를 래핑하여 Docker가 없는 Linux 호스트를 위한
  daemonless, rootless 백엔드를 제공합니다.
- **공유 보안 프로필 레이어** — seccomp 프로필, AppArmor FUSE 프로필,
  Landlock 규칙, 네트워크 egress 정책, 레지스트리 화이트리스트를 모든
  백엔드가 공유하여 모든 런타임이 동일한 정책을 강제합니다.

## Crate 구성

| Crate | 설명 |
|-------|-------------|
| `cherino-macros` | DTO 타입에서 사용하는 `Getters` derive 매크로 |
| `cherino` | `ContainerOps` trait, Docker 백엔드, 보안 프로필, 공유 타입 |
| `cherino-runtime` | Youki/libcontainer OCI 백엔드 (Linux 전용, 비 Linux에서는 스텁) |

## 설치

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

로컬 Docker 데몬을 대상으로 한 최소 사용 예시:

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

## AppArmor 프로필

중첩 컨테이너(컨테이너 내부에서 생성된 컨테이너)는 FUSE AppArmor
프로필이 호스트에 설치되어 있어야 합니다. root로, 호스트당 한 번
설치하세요:

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

레거시 `celestia-plana-fuse` 프로필을 아직 사용하는 호스트는 감지되어
지원 중단 경고와 함께 허용됩니다; 가능하면 새 이름으로 설치하세요.
