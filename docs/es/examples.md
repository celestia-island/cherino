# Ejemplos

Todos los ejemplos compilan contra las API reales de `cherino` /
`cherino-runtime`. Añade primero los crates:

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## Listar contenedores (backend Docker)

El inicio rápido mínimo: conectarse al daemon Docker local y listar los
contenedores en ejecución a través del trait `ContainerOps`.

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

## Crear, iniciar, exec, eliminar

El ciclo de vida completo a través de `ContainerOps`.
`ContainerCreateParams::simple` rellena todos los campos con valores seguros
por defecto; sobrescribe lo que necesites.

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

## Contenedores OCI rootless (backend Youki)

`YoukiManager` de `cherino-runtime` implementa el mismo trait `ContainerOps`,
rootless y sin daemon, en Linux. La misma secuencia de llamadas funciona —
solo difiere la construcción.

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

En plataformas no Linux, `cherino-runtime` compila un stub con el mismo tipo y
la misma implementación del trait cuyas llamadas devuelven errores "only
available on Linux", de modo que el código multiplataforma compila sin
cambios.

## Aplicar un perfil de seguridad

Las cargas de trabajo de la plataforma usan los perfiles predefinidos de
`cherino::security_profile`. Cada `ContainerSecurity` se corresponde
uno a uno con los campos de `ContainerCreateParams`:

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

## Política de egress personalizada

Construye una lista blanca de egress de forma fluida y adjúntala a los
parámetros del contenedor:

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## Una nota sobre el gestor Snowflake

El gestor Snowflake — el orquestador de sandboxes por workspace que ejecuta
las cargas de trabajo de los agentes — **no forma parte de cherino**: vive
aguas abajo en el workspace entelecheia, como consumidor de la capa de runtime
interna (Cosmos). Selecciona su backend a través de `ContainerOps`, con
Youki/libcontainer por defecto (`COSMOS_CONTAINER_RUNTIME=youki`), y superpone
su propia política de egress por workspace sobre
`cherino::security_profile::cosmos()`. El patrón a copiar es el de arriba:
depender del trait `ContainerOps`, construir el backend que el host soporte y
aplicar los perfiles de seguridad compartidos.
