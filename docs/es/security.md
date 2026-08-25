# Seguridad

Cherino aplica defensa en profundidad a los contenedores aislados mediante una
capa compartida de perfiles de seguridad. Los mismos tipos de política son
consumidos por todos los backends, de modo que los runtimes Docker y Youki
aplican restricciones idénticas.

## Perfiles de seguridad

`ContainerSecurity` agrupa la política por carga de trabajo:

- `cap_drop` / `cap_add` — conjuntos de capabilities de Linux;
- `security_opt` — opciones de seguridad de Docker (p. ej.
  `no-new-privileges:true`);
- `egress_policy` — la política de egress de red (ver más abajo).

Los perfiles predefinidos viven en `cherino::security_profile`: `postgres()`,
`scepter()`, `scepter_readonly()` y `cosmos()`. Codifican los valores
endurecidos por defecto de la plataforma — por ejemplo, `scepter()` descarta
`ALL` capabilities y solo añade de vuelta el conjunto mínimo que necesitan sus
subcontenedores.

Con la feature `docker`, `cherino::apply_to_host_config` aplica un
`ContainerSecurity` a un `HostConfig` de Bollard, traduciendo la política a
ajustes nativos de Docker (incluidas las reglas de egress).

## seccomp

`SeccompProfile` / `SeccompProfileData` describen perfiles de filtrado de
syscalls, y `build_security_opts` convierte un perfil en entradas
`security_opt` para la configuración del contenedor. El filtrado seccomp es la
primera línea de defensa: restringe qué syscalls puede realizar una carga de
trabajo aislada en absoluto.

## AppArmor

Los contenedores anidados necesitan montar sistemas de archivos FUSE, algo que
la política AppArmor predeterminada bloquea. Cherino incluye un perfil
dedicado, `celestia-cherino-fuse`
(`crates/cherino/src/apparmor/celestia-cherino-fuse`), que debe instalarse en
el host una sola vez, como root:

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

En tiempo de ejecución, cherino detecta qué perfil está instalado
(`installed_profile_name`) y lo adjunta mediante `fuse_security_opts`. Los
hosts que aún llevan el perfil heredado `celestia-plana-fuse` se aceptan con
una advertencia de obsolescencia.

La válvula de escape `CHERINO_APPARMOR_UNCONFINED` (heredada:
`PLANA_APPARMOR_UNCONFINED`) omite el confinamiento AppArmor en hosts donde no
se puede instalar ningún perfil — cada uso emite un `tracing::warn!`.

## Landlock

`LandlockRules` expresa reglas de acceso al sistema de archivos aplicadas a
través de Landlock de Linux, restringiendo qué rutas puede tocar un proceso
aislado con independencia de los límites del contenedor.

## Control de egress

`EgressPolicy` controla el acceso saliente a la red con tres modos
(`EgressMode`): `DenyAll` (predeterminado), `AllowAll` y `Whitelist`. Las
políticas se construyen de forma fluida:

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

`EgressPolicy::deny_all()` bloquea todo el egress; `entelecheia_default()`
devuelve la lista blanca estándar de la plataforma. En Docker, la política se
materializa mediante fijación de DNS y entradas `extra_hosts`, de modo que solo
los nombres de la lista blanca resuelven a direcciones reales.

## Listas blancas de registries

`RegistryWhitelist` (con `RegistryEntry`) restringe de qué registries de
imágenes se pueden extraer contenedores. Las listas blancas se pueden cargar
desde un archivo (`RegistryWhitelist::load`), analizar desde texto
(`RegistryWhitelist::parse`) o resolver desde un workspace
(`resolve_from_workspace`).

## Operación rootless

El backend `cherino-runtime` (Youki/libcontainer) ejecuta contenedores de
forma rootless y sin daemon: ningún daemon privilegiado guarda el estado de
los contenedores, y las cargas de trabajo aisladas se ejecutan como el usuario
que las invoca — reduciendo la superficie de ataque privilegiada en los hosts
donde se usa.

## Marca y compatibilidad

`cherino` se extrajo del workspace `plana`. Los nombres heredados aún se leen
por compatibilidad, cada uno emitiendo un `tracing::warn!`:

| Heredado (plana / entelecheia) | Nuevo (cherino) |
|--------------------------------|-----------------|
| Perfil AppArmor `celestia-plana-fuse` | `celestia-cherino-fuse` |
| Env `PLANA_APPARMOR_UNCONFINED` | Env `CHERINO_APPARMOR_UNCONFINED` |
| Env `ENTELECHEIA_RUN_DIR` | Env `CHERINO_RUN_DIR` |
| Directorio de ejecución `/tmp/entelecheia/youki` | Directorio de ejecución `/tmp/cherino/youki` |

Las variables genéricas de sustitución (`CONTAINER_RUN_DIR`,
`CONTAINER_ROOTFS_URL`, `CONTAINER_NETWORK`) mantienen precedencia sobre todos
los nombres de marca.
