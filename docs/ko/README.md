<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/master/res/logo/cherino.webp" alt="Cherino" width="240" /></p>

<h1 align="center">Cherino</h1>

<p align="center"><strong>celestia 플랫폼을 위한 통합적이고 런타임에 구애받지 않는 컨테이너 운영 툴킷</strong></p>

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
[日本語](../ja/README.md) ·
**한국어** ·
[Français](../fr/README.md) ·
[Español](../es/README.md) ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

Cherino는 celestia 플랫폼의 컨테이너 운영 툴킷입니다. 샌드박스화된
컨테이너를 생성하고 관리하기 위한 통합적이고 런타임에 구애받지 않는
API를 제공하는 독립형 [Rust](https://www.rust-lang.org/) 라이브러리입니다.

`cherino`는 [`ContainerOps`] trait을 정의합니다 — 컨테이너의 전체
생명주기(생성, 시작, 중지, exec, 파일 복사, 스냅샷, 볼륨, 이미지)와
레퍼런스 Docker 구현, 그리고 공유 보안 프로필 레이어(seccomp, AppArmor,
Landlock, egress 제어, 레지스트리 화이트리스트)를 포함합니다.
`cherino-runtime`은 [libcontainer](https://github.com/containers/youki)
위에 구축된 OCI 네이티브 rootless 백엔드를 추가합니다.

## Crate 구성

| Crate | 설명 |
|-------|-------------|
| [`cherino-macros`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-macros) | DTO 타입에서 사용하는 `Getters` derive 매크로 |
| [`cherino`](https://github.com/celestia-island/cherino/tree/master/crates/cherino) | `ContainerOps` trait, Docker 백엔드, 보안 프로필, 공유 타입 |
| [`cherino-runtime`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-runtime) | Youki/libcontainer OCI 백엔드 (Linux 전용, 비 Linux에서는 스텁) |

## ContainerOps 백엔드

| 백엔드 | 유형 | 플랫폼 | 메커니즘 | 티어 |
|---------|------|----------|-----------|------|
| **Docker** | API | 전체 | Bollard → Docker Engine HTTP API | primary |
| **Youki** | 네이티브 | Linux | libcontainer → OCI rootless 컨테이너 | fallback |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` 셸 호출 | fallback |
| **Apple Container** | CLI | macOS 26+ | `container` CLI (컨테이너당 VM) | fallback |

**Youki는 fallback 티어입니다**: `cherino-runtime`의 libcontainer 백엔드는
Docker가 없는 호스트를 위한 rootless, daemonless 대안으로 유지 관리되며,
기본 오케스트레이션 드라이버가 아닙니다. Docker 백엔드가 기본값이며 가장
검증된 경로입니다.

## Features

`cherino`:

- `docker` *(기본값)* — Bollard 기반 `ContainerManager` 백엔드.
- `cli-backend` — WSLc / Apple Container CLI 어댑터(`cli-backend`
  모듈). 기본적으로 비활성화; Windows/macOS 호스트에서 명시적으로
  활성화하세요.
- `docker-tests` — 실행 중인 Docker 데몬이 필요한 통합 테스트
  (기본적으로 비활성화; `docker`를 포함).

`cherino-runtime`은 `cfg(target_os = "linux")`에서 Linux 백엔드를
자동으로 빌드합니다; 다른 모든 플랫폼에서는 "only available on Linux"
오류를 반환하는 스텁을 컴파일합니다.

## 빠른 시작

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

중첩 컨테이너에 필요한 FUSE AppArmor 프로필 로드(호스트에서, root로,
호스트당 한 번):

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

레거시 `celestia-plana-fuse` 프로필을 아직 사용하는 호스트는 감지되어
지원 중단 경고와 함께 허용됩니다; 가능하면 새 이름으로 설치하세요.

## 브랜딩 및 호환성

`cherino`는 `plana` 워크스페이스에서 추출되었습니다. 다음 레거시 이름들은
호환성을 위해 여전히 읽히며, 각각 `tracing::warn!`을 발생시킵니다:

| 레거시 (plana / entelecheia) | 신규 (cherino) |
|------------------------------|---------------|
| AppArmor 프로필 `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` 환경 변수 | `CHERINO_APPARMOR_UNCONFINED` 환경 변수 |
| `ENTELECHEIA_RUN_DIR` 환경 변수 | `CHERINO_RUN_DIR` 환경 변수 |
| `/tmp/entelecheia/youki` 실행 디렉터리 | `/tmp/cherino/youki` 실행 디렉터리 |

일반적인 재정의 값(`CONTAINER_RUN_DIR`, `CONTAINER_ROOTFS_URL`,
`CONTAINER_NETWORK`)은 모든 브랜드 이름보다 우선합니다.

## AI 생성 공개

이 저장소의 코드는 상당 부분 AI에 의해 생성되었으며
[SySL-1.0](../../LICENSE) 라이선스로 배포됩니다. 라이선스 전문, 이 저장소의
[LICENSE](../../LICENSE)에 첨부된 모델 공개 사항, 그리고 FAQ는 `sysl`
저장소(<https://github.com/celestia-island/sysl>)를 참조하세요.

## 라이선스

[SySL-1.0](../../LICENSE) 라이선스로 배포됩니다. 이 소프트웨어를
사용하면 AI 생성 공개 및 위험 인지 약관에 동의하는 것으로 간주됩니다.
