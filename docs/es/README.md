<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/master/res/logo/cherino.webp" alt="Cherino" width="240" /></p>

<h1 align="center">Cherino</h1>

<p align="center"><strong>Kit de herramientas unificado e independiente del runtime para operaciones con contenedores en la plataforma celestia</strong></p>

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
[한국어](../ko/README.md) ·
[Français](../fr/README.md) ·
**Español** ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

Cherino es un kit de herramientas de operaciones con contenedores para la
plataforma celestia: una biblioteca de [Rust](https://www.rust-lang.org/)
independiente que proporciona una API unificada e independiente del runtime
para crear y gestionar contenedores en entorno aislado (sandbox).

`cherino` define el trait [`ContainerOps`] — el ciclo de vida completo del
contenedor (crear, iniciar, detener, exec, copia de archivos, instantáneas,
volúmenes, imágenes) — además de una implementación de referencia para Docker
y una capa compartida de perfiles de seguridad (seccomp, AppArmor, Landlock,
control de egress, listas blancas de registries). `cherino-runtime` añade un
backend OCI nativo y rootless construido sobre
[libcontainer](https://github.com/containers/youki).

## Estructura de crates

| Crate | Descripción |
|-------|-------------|
| [`cherino-macros`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-macros) | Macro derive `Getters` utilizada por los tipos DTO |
| [`cherino`](https://github.com/celestia-island/cherino/tree/master/crates/cherino) | Trait `ContainerOps`, backend Docker, perfiles de seguridad, tipos compartidos |
| [`cherino-runtime`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-runtime) | Backend OCI Youki/libcontainer (solo Linux, stubs en otros sistemas) |

## Backends de ContainerOps

| Backend | Tipo | Plataforma | Mecanismo | Nivel |
|---------|------|----------|-----------|------|
| **Docker** | API | Todas | Bollard → Docker Engine HTTP API | principal |
| **Youki** | Nativo | Linux | libcontainer → contenedores OCI rootless | reserva |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell-out | reserva |
| **Apple Container** | CLI | macOS 26+ | `container` CLI (una VM por contenedor) | reserva |

**Youki es un nivel de reserva**: el backend libcontainer de `cherino-runtime`
se mantiene como una alternativa rootless y sin daemon para hosts sin Docker,
no como el motor de orquestación principal. El backend Docker es el predeterminado
y el camino más probado en producción.

## Features

`cherino`:

- `docker` *(predeterminado)* — el backend `ContainerManager` basado en Bollard.
- `cli-backend` — los adaptadores CLI de WSLc / Apple Container (módulo
  `cli-backend`). Desactivado por defecto; actívalo explícitamente en hosts
  Windows/macOS.
- `docker-tests` — pruebas de integración que requieren un daemon Docker en
  ejecución (desactivado por defecto; implica `docker`).

`cherino-runtime` compila su backend de Linux automáticamente en
`cfg(target_os = "linux")`; todas las demás plataformas compilan un stub que
devuelve errores "only available on Linux".

## Inicio rápido

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

Carga del perfil AppArmor de FUSE requerido por los contenedores anidados
(en el host, como root, una vez por host):

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Los hosts que aún llevan el perfil heredado `celestia-plana-fuse` se detectan
y se aceptan con una advertencia de obsolescencia; instala el nuevo nombre
cuando puedas.

## Marca y compatibilidad

`cherino` se extrajo del workspace `plana`. Los siguientes nombres heredados
aún se leen por compatibilidad, cada uno emitiendo un `tracing::warn!`:

| Heredado (plana / entelecheia) | Nuevo (cherino) |
|--------------------------------|-----------------|
| Perfil AppArmor `celestia-plana-fuse` | `celestia-cherino-fuse` |
| Env `PLANA_APPARMOR_UNCONFINED` | Env `CHERINO_APPARMOR_UNCONFINED` |
| Env `ENTELECHEIA_RUN_DIR` | Env `CHERINO_RUN_DIR` |
| Directorio de ejecución `/tmp/entelecheia/youki` | Directorio de ejecución `/tmp/cherino/youki` |

Las variables genéricas de sustitución (`CONTAINER_RUN_DIR`,
`CONTAINER_ROOTFS_URL`, `CONTAINER_NETWORK`) mantienen precedencia sobre todos
los nombres de marca.

## Divulgación de generación por IA

El código de este repositorio está generado sustancialmente por IA y se
licencia bajo la licencia [SySL-1.0](../../LICENSE). Consulta el repositorio
`sysl` (<https://github.com/celestia-island/sysl>) para el texto de la
licencia, la divulgación del modelo adjunta al [LICENSE](../../LICENSE) de
este repositorio y las preguntas frecuentes.

## Licencia

Licenciado bajo la licencia [SySL-1.0](../../LICENSE). Al usar este software
aceptas sus términos de divulgación de generación por IA y de reconocimiento
de riesgos.
