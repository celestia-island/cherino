# Безопасность

Cherino обеспечивает эшелонированную защиту изолированных контейнеров через
общий слой профилей безопасности. Одни и те же типы политик используются
каждым бэкендом, поэтому рантаймы Docker и Youki применяют идентичные
ограничения.

## Профили безопасности

`ContainerSecurity` объединяет политику для конкретной рабочей нагрузки:

- `cap_drop` / `cap_add` — наборы Linux capabilities;
- `security_opt` — опции безопасности Docker (например, `no-new-privileges:true`);
- `egress_policy` — политика сетевого egress (см. ниже).

Готовые профили находятся в `cherino::security_profile`: `postgres()`,
`scepter()`, `scepter_readonly()` и `cosmos()`. Они кодируют усиленные
значения по умолчанию платформы — например, `scepter()` сбрасывает `ALL`
capabilities и добавляет обратно только минимальный набор, необходимый его
вложенным контейнерам.

С feature `docker` функция `cherino::apply_to_host_config` применяет
`ContainerSecurity` к `HostConfig` из Bollard, переводя политику в
Docker-нативные настройки (включая правила egress).

## seccomp

`SeccompProfile` / `SeccompProfileData` описывают профили фильтрации системных
вызовов, а `build_security_opts` преобразует профиль в записи `security_opt`
для конфигурации контейнера. Фильтрация seccomp — первая линия обороны: она
ограничивает, какие системные вызовы изолированная рабочая нагрузка вообще
может выполнять.

## AppArmor

Вложенным контейнерам нужно монтировать файловые системы FUSE, что
блокируется политикой AppArmor по умолчанию. Cherino поставляет выделенный
профиль `celestia-cherino-fuse`
(`crates/cherino/src/apparmor/celestia-cherino-fuse`), который нужно один раз
установить на хосте от root:

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Во время выполнения cherino определяет, какой профиль установлен
(`installed_profile_name`), и подключает его через `fuse_security_opts`. Хосты,
на которых всё ещё установлен устаревший профиль `celestia-plana-fuse`,
принимаются с предупреждением об устаревании.

Аварийный выход `CHERINO_APPARMOR_UNCONFINED` (устаревший:
`PLANA_APPARMOR_UNCONFINED`) пропускает ограничение AppArmor для хостов, где
нельзя установить ни один профиль — каждое использование сопровождается
`tracing::warn!`.

## Landlock

`LandlockRules` выражает правила доступа к файловой системе, применяемые через
Linux Landlock, ограничивая, к каким путям может обращаться изолированный
процесс, независимо от границ контейнера.

## Контроль egress

`EgressPolicy` управляет исходящим сетевым доступом с тремя режимами
(`EgressMode`): `DenyAll` (по умолчанию), `AllowAll` и `Whitelist`. Политики
строятся в fluent-стиле:

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

`EgressPolicy::deny_all()` блокирует весь egress; `entelecheia_default()`
возвращает стандартный белый список платформы. В Docker политика реализуется
через закрепление DNS и записи `extra_hosts`, так что только имена из белого
списка разрешаются в реальные адреса.

## Белые списки реестров

`RegistryWhitelist` (вместе с `RegistryEntry`) ограничивает, из каких реестров
образов можно загружать контейнеры. Белые списки можно загрузить из файла
(`RegistryWhitelist::load`), разобрать из текста (`RegistryWhitelist::parse`)
или разрешить из workspace (`resolve_from_workspace`).

## Работа без root

Бэкенд `cherino-runtime` (Youki/libcontainer) запускает контейнеры без root и
без демона: ни один привилегированный демон не хранит состояние контейнеров, а
изолированные рабочие нагрузки выполняются от имени вызывающего пользователя —
это сокращает привилегированную поверхность атаки на хостах, где он
используется.

## Брендинг и совместимость

`cherino` был выделен из workspace `plana`. Устаревшие имена по-прежнему
считываются ради совместимости, каждое с выводом `tracing::warn!`:

| Устаревшее (plana / entelecheia) | Новое (cherino) |
|------------------------------|---------------|
| Профиль AppArmor `celestia-plana-fuse` | `celestia-cherino-fuse` |
| env `PLANA_APPARMOR_UNCONFINED` | env `CHERINO_APPARMOR_UNCONFINED` |
| env `ENTELECHEIA_RUN_DIR` | env `CHERINO_RUN_DIR` |
| каталог запуска `/tmp/entelecheia/youki` | каталог запуска `/tmp/cherino/youki` |

Общие переопределения (`CONTAINER_RUN_DIR`, `CONTAINER_ROOTFS_URL`,
`CONTAINER_NETWORK`) имеют приоритет над всеми брендированными именами.
