# Введение

Cherino — набор инструментов для операций с контейнерами платформы celestia:
автономная библиотека Rust, предоставляющая унифицированный, независимый от
рантайма API для создания и управления изолированными контейнерами.

Каждая изолированная рабочая нагрузка в платформе celestia выполняется внутри
эфемерного изолированного контейнера. Нижестоящий код зависит только от трейта
`ContainerOps`, а не от конкретного рантайма — поэтому одна и та же логика
оркестрации работает с Docker, rootless OCI-рантаймом или бэкендом на основе
CLI.

## Что предоставляет

- **Трейт `ContainerOps`** — полный жизненный цикл контейнера: создание,
  запуск, остановка, удаление, перезапуск, exec, копирование файлов в/из
  контейнера, снимки и диффы файловой системы, тома и образы.
- **Эталонный бэкенд Docker** — `ContainerManager` управляет Docker Engine
  через HTTP API с помощью [Bollard](https://crates.io/crates/bollard)
  (feature `docker`, включён по умолчанию).
- **Rootless OCI-бэкенд** — crate `cherino-runtime` оборачивает
  [libcontainer](https://github.com/containers/youki) (библиотеку, лежащую в
  основе OCI-рантайма Youki) как бэкенд без демона и без root для Linux-хостов
  без Docker.
- **Общий слой профилей безопасности** — профили seccomp, профиль AppArmor для
  FUSE, правила Landlock, политики сетевого egress и белые списки реестров,
  общие для всех бэкендов, так что каждый рантайм применяет одну и ту же
  политику.

## Структура crate

| Crate | Описание |
|-------|-------------|
| `cherino-macros` | derive-макрос `Getters`, используемый DTO-типами |
| `cherino` | трейт `ContainerOps`, бэкенд Docker, профили безопасности, общие типы |
| `cherino-runtime` | OCI-бэкенд Youki/libcontainer (только Linux, на остальных платформах — заглушки) |

## Установка

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

Минимальный пример использования с локальным Docker-демоном:

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

## Профиль AppArmor

Вложенным контейнерам (контейнерам, созданным изнутри контейнера) необходим
профиль AppArmor для FUSE, установленный на хосте от root, один раз на хост:

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Хосты, на которых всё ещё установлен устаревший профиль `celestia-plana-fuse`,
обнаруживаются и принимаются с предупреждением об устаревании; при возможности
установите профиль с новым именем.
