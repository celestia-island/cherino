# Backends

Cherino trata el aislamiento de contenedores como un problema de *política
sobre mecanismo*: la plataforma prescribe *qué* está permitido (mediante
perfiles de seguridad), y cada backend lo traduce a su mecanismo nativo
(configuraciones de host de Docker o hooks del spec OCI). Todos los backends
implementan el mismo trait `ContainerOps`.

| Backend | Tipo | Plataforma | Mecanismo | Nivel |
|---------|------|----------|-----------|------|
| **Docker** | API | Todas | Bollard → Docker Engine HTTP API | principal |
| **Youki** | Nativo | Linux | libcontainer → contenedores OCI rootless | reserva |
| **WSLc** | CLI | Windows | `wslc.exe` / `container.exe` shell-out | reserva |
| **Apple Container** | CLI | macOS 26+ | `container` CLI (una VM por contenedor) | reserva |

## Docker (principal)

El backend Docker es el predeterminado y el camino más probado en producción.
`ContainerManager` se conecta al daemon Docker local a través de Bollard e
implementa toda la superficie de `ContainerOps`, incluyendo exec, copia de
archivos, instantáneas, volúmenes y gestión de imágenes. Está disponible en
todas las plataformas detrás de la feature `docker`, activada por defecto.

```rust
use cherino::ContainerManager;

let mgr = ContainerManager::new()?; // local daemon
// or, against a specific socket:
let mgr = ContainerManager::new_with_socket("/var/run/docker.sock")?;
```

## Youki / libcontainer (reserva, Linux)

El crate `cherino-runtime` proporciona `YoukiManager`, un backend nativo OCI
construido sobre libcontainer. Es **rootless y sin daemon**: los contenedores
se ejecutan como el usuario que los invoca sin ningún servicio en segundo
plano, lo que lo convierte en la reserva para hosts donde no hay un daemon
Docker disponible o permitido.

`cherino-runtime` compila su backend real automáticamente en
`cfg(target_os = "linux")`. En cualquier otra plataforma compila un stub cuyas
llamadas devuelven errores "only available on Linux", de modo que el código
multiplataforma puede depender de él incondicionalmente.

**Youki es un nivel de reserva**: se mantiene como una alternativa rootless,
no como el motor de orquestación principal. Prefiere el backend Docker siempre
que haya un daemon disponible.

## Backends CLI (reserva, Windows / macOS)

Detrás de la feature opcional `cli-backend`, cherino incluye adaptadores CLI
para runtimes de contenedores que no exponen una API local estable:

- **WSLc** en Windows — invoca `wslc.exe` / `container.exe` por shell.
- **Apple Container** en macOS 26+ — controla el CLI `container`, que ejecuta
  una VM ligera por contenedor.

`cli-backend` está desactivada por defecto; actívala explícitamente en hosts
Windows/macOS:

```toml
[dependencies]
cherino = { version = "0.1", features = ["cli-backend"] }
```

## Arquitectura de runtime en dos capas

La plataforma celestia utiliza dos runtimes de contenedores distintos en
capas diferentes:

| Capa | Runtime | Utilizado por |
|------|---------|---------------|
| **Externa** (orquestación) | Docker/Podman | Health check del TUI, daemon scepter, gestor de servidores |
| **Interna** (sandbox cosmos) | Youki/libcontainer | Gestor Snowflake, estado del agente Neikos |

La capa externa gestiona los contenedores de infraestructura (scepter,
postgres) mediante la API de Docker/Podman; estos necesitan funciones de
orquestación completas — redes, volúmenes persistentes, health checks,
composición de múltiples contenedores. La capa interna (Cosmos) se ejecuta
*dentro* del contenedor scepter y utiliza Youki para crear contenedores
aislados ligeros y de arranque rápido para la ejecución de agentes, cada uno
con su propio perfil seccomp y límites de recursos.

## Feature flags

`cherino`:

- `docker` *(predeterminado)* — el backend `ContainerManager` basado en Bollard.
- `cli-backend` — los adaptadores CLI de WSLc / Apple Container. Desactivada
  por defecto.
- `docker-tests` — pruebas de integración que requieren un daemon Docker en
  ejecución (desactivada por defecto; implica `docker`).
