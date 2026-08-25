<h1 align="center">Cherino</h1>

<p align="center"><strong>Унифицированный, независимый от рантайма набор инструментов для операций с контейнерами платформы celestia</strong></p>

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
[Español](../es/README.md) ·
**Русский** ·
[العربية](../ar/README.md)

</div>

Cherino — набор инструментов для операций с контейнерами платформы celestia:
автономная библиотека [Rust](https://www.rust-lang.org/), предоставляющая
унифицированный, независимый от рантайма API для создания и управления
изолированными контейнерами.

`cherino` определяет трейт [`ContainerOps`] — полный жизненный цикл контейнера
(создание, запуск, остановка, exec, копирование файлов, снимки, тома, образы) —
а также эталонную реализацию для Docker и общий слой профилей безопасности
(seccomp, AppArmor, Landlock, контроль egress, белые списки реестров).
`cherino-runtime` добавляет OCI-нативный rootless-бэкенд, построенный на
[libcontainer](https://github.com/containers/youki).

## Структура crate

| Crate | Описание |
|-------|-------------|
| [`cherino-macros`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-macros) | derive-макрос `Getters`, используемый DTO-типами |
| [`cherino`](https://github.com/celestia-island/cherino/tree/master/crates/cherino) | трейт `ContainerOps`, бэкенд Docker, профили безопасности, общие типы |
| [`cherino-runtime`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-runtime) | OCI-бэкенд Youki/libcontainer (только Linux, на остальных платформах — заглушки) |

## Бэкенды ContainerOps

| Бэкенд | Тип | Платформа | Механизм | Уровень |
|---------|------|----------|-----------|------|
| **Docker** | API | Все | Bollard → Docker Engine HTTP API | основной |
| **Youki** | Native | Linux | libcontainer → rootless-контейнеры OCI | резервный |
| **WSLc** | CLI | Windows | вызовы `wslc.exe` / `container.exe` | резервный |
| **Apple Container** | CLI | macOS 26+ | CLI `container` (по VM на контейнер) | резервный |

**Youki — резервный уровень**: бэкенд libcontainer в `cherino-runtime`
поддерживается как rootless-альтернатива без демона для хостов без Docker, а не
как основной драйвер оркестрации. Бэкенд Docker используется по умолчанию и
является наиболее проверенным в бою путём.

## Features

`cherino`:

- `docker` *(по умолчанию)* — бэкенд `ContainerManager` на базе Bollard.
- `cli-backend` — CLI-адаптеры WSLc / Apple Container (модуль `cli-backend`).
  По умолчанию выключены; включайте явно на хостах Windows/macOS.
- `docker-tests` — интеграционные тесты, требующие работающий Docker-демон
  (по умолчанию выключены; подразумевают `docker`).

`cherino-runtime` автоматически собирает свой Linux-бэкенд на
`cfg(target_os = "linux")`; на всех остальных платформах компилируется
заглушка, возвращающая ошибки «доступно только на Linux».

## Быстрый старт

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

Установка профиля AppArmor для FUSE, необходимого вложенным контейнерам
(на хосте, от root, один раз на хост):

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Хосты, на которых всё ещё установлен устаревший профиль `celestia-plana-fuse`,
обнаруживаются и принимаются с предупреждением об устаревании; при возможности
установите профиль с новым именем.

## Брендинг и совместимость

`cherino` был выделен из workspace `plana`. Следующие устаревшие имена
по-прежнему считываются ради совместимости, каждое с выводом `tracing::warn!`:

| Устаревшее (plana / entelecheia) | Новое (cherino) |
|------------------------------|---------------|
| Профиль AppArmor `celestia-plana-fuse` | `celestia-cherino-fuse` |
| env `PLANA_APPARMOR_UNCONFINED` | env `CHERINO_APPARMOR_UNCONFINED` |
| env `ENTELECHEIA_RUN_DIR` | env `CHERINO_RUN_DIR` |
| каталог запуска `/tmp/entelecheia/youki` | каталог запуска `/tmp/cherino/youki` |

Общие переопределения (`CONTAINER_RUN_DIR`, `CONTAINER_ROOTFS_URL`,
`CONTAINER_NETWORK`) имеют приоритет над всеми брендированными именами.

## Уведомление об ИИ-генерации

Код этого репозитория в значительной степени сгенерирован ИИ и лицензирован под
лицензией [SySL-1.0](../../LICENSE). Текст лицензии, уведомление о модели,
приложенное к [LICENSE](../../LICENSE) этого репозитория, и FAQ см. в
репозитории `sysl` (<https://github.com/celestia-island/sysl>).

## Лицензия

Лицензировано под лицензией [SySL-1.0](../../LICENSE). Используя это
программное обеспечение, вы принимаете его условия уведомления об
ИИ-генерации и осознания рисков.
