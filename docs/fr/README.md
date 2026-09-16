<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/master/res/logo/cherino.webp" alt="Cherino" width="240" /></p>

<h1 align="center">Cherino</h1>

<p align="center"><strong>Boîte à outils unifiée et indépendante du runtime pour les opérations sur les conteneurs de la plateforme celestia</strong></p>

<div align="center">

[![License: SySL-1.0](https://img.shields.io/badge/License-SySL--1.0-blue.svg)](https://sysl.celestia.world)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fcherino-blue.svg)](https://github.com/celestia-island/cherino)
[![Docs](https://img.shields.io/badge/docs-cherino.docs.celestia.world-blue)](https://cherino.docs.celestia.world)
[![docs.rs](https://docs.rs/cherino/badge.svg)](https://docs.rs/cherino)
[![Checks](https://img.shields.io/github/actions/workflow/status/celestia-island/cherino/checks.yml)](https://github.com/celestia-island/cherino/actions/workflows/checks.yml)

</div>

<div align="center">

[English](../../README.md) ·
[简体中文](../zh-Hans/README.md) ·
[繁體中文](../zh-Hant/README.md) ·
[日本語](../ja/README.md) ·
[한국어](../ko/README.md) ·
**Français** ·
[Español](../es/README.md) ·
[Русский](../ru/README.md) ·
[العربية](../ar/README.md)

</div>

Cherino est une boîte à outils d'opérations sur les conteneurs pour la
plateforme celestia — une bibliothèque [Rust](https://www.rust-lang.org/)
autonome fournissant une API unifiée, indépendante du runtime, pour créer et
gérer des conteneurs sandboxés.

`cherino` définit le trait [`ContainerOps`] — le cycle de vie complet des
conteneurs (création, démarrage, arrêt, exec, copie de fichiers, snapshots,
volumes, images) — ainsi qu'une implémentation Docker de référence et une
couche partagée de profils de sécurité (seccomp, AppArmor, Landlock, contrôle
de l'egress, whitelist des registres). `cherino-runtime` ajoute un backend
OCI natif et rootless construit sur
[libcontainer](https://github.com/containers/youki).

## Organisation des crates

| Crate | Description |
|-------|-------------|
| [`cherino-macros`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-macros) | Macro derive `Getters` utilisée par les types DTO |
| [`cherino`](https://github.com/celestia-island/cherino/tree/master/crates/cherino) | Trait `ContainerOps`, backend Docker, profils de sécurité, types partagés |
| [`cherino-runtime`](https://github.com/celestia-island/cherino/tree/master/crates/cherino-runtime) | Backend OCI Youki/libcontainer (Linux uniquement, stubs pour les autres plateformes) |

## Backends ContainerOps

| Backend | Type | Plateforme | Mécanisme | Niveau |
|---------|------|----------|-----------|------|
| **Docker** | API | Toutes | Bollard → Docker Engine HTTP API | primaire |
| **Youki** | Natif | Linux | libcontainer → conteneurs OCI rootless | repli |
| **WSLc** | CLI | Windows | shell-out vers `wslc.exe` / `container.exe` | repli |
| **Apple Container** | CLI | macOS 26+ | CLI `container` (une VM par conteneur) | repli |

**Youki est un backend de repli** : le backend libcontainer de
`cherino-runtime` est maintenu comme une alternative rootless et sans daemon
pour les hôtes dépourvus de Docker, et non comme le moteur d'orchestration
principal. Le backend Docker est le choix par défaut et le chemin le plus
éprouvé.

## Features

`cherino` :

- `docker` *(par défaut)* — le backend `ContainerManager` basé sur Bollard.
- `cli-backend` — les adaptateurs CLI WSLc / Apple Container (module
  `cli-backend`). Désactivée par défaut ; à activer explicitement sur les
  hôtes Windows/macOS.
- `docker-tests` — tests d'intégration nécessitant un daemon Docker actif
  (désactivée par défaut ; implique `docker`).

`cherino-runtime` compile automatiquement son backend Linux sous
`cfg(target_os = "linux")` ; toutes les autres plateformes compilent un stub
qui renvoie des erreurs « only available on Linux ».

## Démarrage rapide

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

Chargement du profil AppArmor FUSE requis par les conteneurs imbriqués (hôte,
root, une fois par hôte) :

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Les hôtes portant encore l'ancien profil `celestia-plana-fuse` sont détectés
et acceptés avec un avertissement de dépréciation ; installez le nouveau nom
dès que possible.

## Marque et compatibilité

`cherino` a été extrait du workspace `plana`. Les anciens noms suivants sont
toujours lus pour compatibilité, chacun émettant un `tracing::warn!` :

| Ancien nom (plana / entelecheia) | Nouveau nom (cherino) |
|----------------------------------|-----------------------|
| Profil AppArmor `celestia-plana-fuse` | `celestia-cherino-fuse` |
| env `PLANA_APPARMOR_UNCONFINED` | env `CHERINO_APPARMOR_UNCONFINED` |
| env `ENTELECHEIA_RUN_DIR` | env `CHERINO_RUN_DIR` |
| répertoire d'exécution `/tmp/entelecheia/youki` | répertoire d'exécution `/tmp/cherino/youki` |

Les surcharges génériques (`CONTAINER_RUN_DIR`, `CONTAINER_ROOTFS_URL`,
`CONTAINER_NETWORK`) gardent la priorité sur tous les noms de marque.

## Transparence sur la génération par IA

Le code de ce dépôt est substantiellement généré par IA et est sous licence
[SySL-1.0](../../LICENSE). Consultez le dépôt `sysl`
(<https://github.com/celestia-island/sysl>) pour le texte de la licence, la
déclaration relative aux modèles annexée au [LICENSE](../../LICENSE) de ce
dépôt, ainsi que la FAQ.

## Licence

Sous licence [SySL-1.0](../../LICENSE). En utilisant ce logiciel, vous
acceptez ses conditions de transparence sur la génération par IA et de
reconnaissance des risques.
