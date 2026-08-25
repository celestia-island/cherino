# Introducción

Cherino es el kit de herramientas de operaciones con contenedores de la
plataforma celestia: una biblioteca de Rust independiente que proporciona una
API unificada e independiente del runtime para crear y gestionar contenedores
en entorno aislado (sandbox).

Cada carga de trabajo aislada de la plataforma celestia se ejecuta dentro de
un contenedor efímero y en sandbox. El código aguas abajo solo depende del
trait `ContainerOps`, nunca de un runtime concreto — de modo que la misma
lógica de orquestación funciona contra Docker, un runtime OCI rootless o un
backend basado en CLI.

## Qué proporciona

- **El trait `ContainerOps`** — el ciclo de vida completo del contenedor:
  crear, iniciar, detener, eliminar, reiniciar, exec, copia de archivos de
  entrada/salida, instantáneas y diffs del sistema de archivos, volúmenes e
  imágenes.
- **Un backend Docker de referencia** — `ContainerManager` controla la HTTP API
  de Docker Engine a través de [Bollard](https://crates.io/crates/bollard) (la
  feature `docker`, activada por defecto).
- **Un backend OCI rootless** — el crate `cherino-runtime` envuelve
  [libcontainer](https://github.com/containers/youki) (la biblioteca subyacente
  al runtime OCI Youki) como un backend sin daemon y rootless para hosts Linux
  sin Docker.
- **Una capa compartida de perfiles de seguridad** — perfiles seccomp, un
  perfil AppArmor de FUSE, reglas Landlock, políticas de egress de red y listas
  blancas de registries, compartidas por todos los backends para que cada
  runtime aplique la misma política.

## Estructura de crates

| Crate | Descripción |
|-------|-------------|
| `cherino-macros` | Macro derive `Getters` utilizada por los tipos DTO |
| `cherino` | Trait `ContainerOps`, backend Docker, perfiles de seguridad, tipos compartidos |
| `cherino-runtime` | Backend OCI Youki/libcontainer (solo Linux, stubs en otros sistemas) |

## Instalación

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

Uso mínimo contra el daemon Docker local:

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

## Perfil AppArmor

Los contenedores anidados (contenedores creados desde dentro de un contenedor)
necesitan el perfil AppArmor de FUSE instalado en el host, como root, una vez
por host:

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Los hosts que aún llevan el perfil heredado `celestia-plana-fuse` se detectan
y se aceptan con una advertencia de obsolescencia; instala el nuevo nombre
cuando puedas.
