# Примеры

Все примеры компилируются с реальными API `cherino` / `cherino-runtime`.
Сначала добавьте crate:

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## Список контейнеров (бэкенд Docker)

Минимальный быстрый старт: подключение к локальному Docker-демону и вывод
списка запущенных контейнеров через трейт `ContainerOps`.

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

## Создание, запуск, exec, удаление

Полный жизненный цикл через `ContainerOps`. `ContainerCreateParams::simple`
заполняет каждое поле безопасным значением по умолчанию; переопределяйте то,
что нужно.

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

## Rootless OCI-контейнеры (бэкенд Youki)

`YoukiManager` из `cherino-runtime` реализует тот же трейт `ContainerOps`,
без root и без демона, на Linux. Идентичная последовательность вызовов
работает — отличается только конструирование.

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

На платформах, отличных от Linux, `cherino-runtime` компилирует заглушку с тем
же типом и той же реализацией трейта, чьи вызовы возвращают ошибки «доступно
только на Linux», так что кроссплатформенный код компилируется без изменений.

## Применение профиля безопасности

Рабочие нагрузки платформы используют готовые профили из
`cherino::security_profile`. Каждый `ContainerSecurity` отображается один в
один на поля `ContainerCreateParams`:

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

## Пользовательская политика egress

Постройте белый список egress в fluent-стиле и прикрепите его к параметрам
контейнера:

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## Замечание о Snowflake manager

Snowflake manager — оркестратор песочниц уровня workspace, выполняющий
рабочие нагрузки агентов, — **не является частью cherino**: он находится
ниже по потоку, в workspace entelecheia, как потребитель внутреннего
рантайм-слоя (Cosmos). Он выбирает свой бэкенд через `ContainerOps`, по
умолчанию используя Youki/libcontainer (`COSMOS_CONTAINER_RUNTIME=youki`), и
накладывает свою собственную egress-политику уровня workspace поверх
`cherino::security_profile::cosmos()`. Паттерн, который стоит копировать, —
тот, что показан выше: зависеть от трейта `ContainerOps`, конструировать тот
бэкенд, который поддерживает хост, и применять общие профили безопасности.
