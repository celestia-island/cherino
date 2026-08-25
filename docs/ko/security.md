# 보안

Cherino는 공유 보안 프로필 레이어를 통해 샌드박스 컨테이너에 대한
심층 방어(defence-in-depth)를 강제합니다. 동일한 정책 타입이 모든
백엔드에서 소비되므로, Docker와 Youki 런타임은 동일한 제한을 강제합니다.

## 보안 프로필

`ContainerSecurity`는 워크로드별 정책을 묶습니다:

- `cap_drop` / `cap_add` — Linux capability 집합;
- `security_opt` — Docker 보안 옵션 (예: `no-new-privileges:true`);
- `egress_policy` — 네트워크 egress 정책 (아래 참조).

미리 만들어진 프로필은 `cherino::security_profile`에 있습니다:
`postgres()`, `scepter()`, `scepter_readonly()`, `cosmos()`. 이들은
플랫폼의 강화된 기본값을 인코딩합니다 — 예를 들어 `scepter()`는 `ALL`
capability를 제거하고 하위 컨테이너에 필요한 최소 집합만 다시
추가합니다.

`docker` feature와 함께, `cherino::apply_to_host_config`는
`ContainerSecurity`를 Bollard `HostConfig`에 적용하여 정책을
Docker 네이티브 설정(egress 규칙 포함)으로 변환합니다.

## seccomp

`SeccompProfile` / `SeccompProfileData`는 syscall 필터링 프로필을
기술하며, `build_security_opts`는 프로필을 컨테이너 설정용
`security_opt` 항목으로 렌더링합니다. seccomp 필터링은 첫 번째
방어선입니다: 샌드박스 워크로드가 어떤 syscall을 호출할 수 있는지
자체를 제한합니다.

## AppArmor

중첩 컨테이너는 FUSE 파일시스템을 마운트해야 하지만, 기본 AppArmor
정책은 이를 차단합니다. Cherino는 전용 프로필
`celestia-cherino-fuse`(`crates/cherino/src/apparmor/celestia-cherino-fuse`)를
제공하며, 호스트에 root로 한 번 설치해야 합니다:

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

런타임에 cherino는 설치된 프로필을 감지하고(`installed_profile_name`)
`fuse_security_opts`를 통해 연결합니다. 레거시 `celestia-plana-fuse`
프로필을 아직 사용하는 호스트는 지원 중단 경고와 함께 허용됩니다.

우회 수단인 `CHERINO_APPARMOR_UNCONFINED`(레거시:
`PLANA_APPARMOR_UNCONFINED`)는 프로필을 설치할 수 없는 호스트에서
AppArmor 격리를 건너뜁니다 — 사용할 때마다 `tracing::warn!`이
발생합니다.

## Landlock

`LandlockRules`는 Linux Landlock을 통해 강제되는 파일시스템 접근
규칙을 표현하며, 컨테이너 경계와 무관하게 샌드박스 프로세스가 접근할
수 있는 경로를 제한합니다.

## Egress 제어

`EgressPolicy`는 세 가지 모드(`EgressMode`)로 아웃바운드 네트워크
접근을 제어합니다: `DenyAll`(기본값), `AllowAll`, `Whitelist`. 정책은
플루언트하게 구성합니다:

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

`EgressPolicy::deny_all()`은 모든 egress를 차단하고,
`entelecheia_default()`는 플랫폼의 표준 허용 목록을 반환합니다.
Docker에서는 DNS 고정(pinning)과 `extra_hosts` 항목을 통해 정책이
구현되어, 화이트리스트에 있는 이름만 실제 주소로 해석됩니다.

## 레지스트리 화이트리스트

`RegistryWhitelist`(`RegistryEntry`와 함께)는 컨테이너를 풀할 수 있는
이미지 레지스트리를 제한합니다. 화이트리스트는 파일에서 로드하거나
(`RegistryWhitelist::load`), 텍스트에서 파싱하거나
(`RegistryWhitelist::parse`), 워크스페이스에서 해석할 수 있습니다
(`resolve_from_workspace`).

## Rootless 운영

`cherino-runtime`(Youki/libcontainer) 백엔드는 컨테이너를 rootless이자
daemonless로 실행합니다: 컨테이너 상태를 보유하는 특권 데몬이 없고,
샌드박스 워크로드는 호출한 사용자 권한으로 실행됩니다 — 사용되는
호스트에서 특권 공격 표면을 줄여줍니다.

## 브랜딩 및 호환성

`cherino`는 `plana` 워크스페이스에서 추출되었습니다. 레거시 이름들은
호환성을 위해 여전히 읽히며, 각각 `tracing::warn!`을 발생시킵니다:

| 레거시 (plana / entelecheia) | 신규 (cherino) |
|------------------------------|---------------|
| AppArmor 프로필 `celestia-plana-fuse` | `celestia-cherino-fuse` |
| `PLANA_APPARMOR_UNCONFINED` 환경 변수 | `CHERINO_APPARMOR_UNCONFINED` 환경 변수 |
| `ENTELECHEIA_RUN_DIR` 환경 변수 | `CHERINO_RUN_DIR` 환경 변수 |
| `/tmp/entelecheia/youki` 실행 디렉터리 | `/tmp/cherino/youki` 실행 디렉터리 |

일반적인 재정의 값(`CONTAINER_RUN_DIR`, `CONTAINER_ROOTFS_URL`,
`CONTAINER_NETWORK`)은 모든 브랜드 이름보다 우선합니다.
