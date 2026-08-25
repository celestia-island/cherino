# Introduction

Cherino est la boîte à outils d'opérations sur les conteneurs de la
plateforme celestia : une bibliothèque Rust autonome qui fournit une API
unifiée, indépendante du runtime, pour créer et gérer des conteneurs
sandboxés.

Chaque charge de travail sandboxée de la plateforme celestia s'exécute dans
un conteneur éphémère et sandboxé. Le code en aval ne dépend que du trait
`ContainerOps`, jamais d'un runtime concret — la même logique d'orchestration
fonctionne donc avec Docker, un runtime OCI rootless ou un backend basé sur
une CLI.

## Ce qu'il fournit

- **Le trait `ContainerOps`** — le cycle de vie complet des conteneurs :
  création, démarrage, arrêt, suppression, redémarrage, exec, copie de
  fichiers dans les deux sens, snapshots et diffs du système de fichiers,
  volumes et images.
- **Un backend Docker de référence** — `ContainerManager` pilote l'API HTTP
  du Docker Engine via [Bollard](https://crates.io/crates/bollard) (la
  feature `docker`, activée par défaut).
- **Un backend OCI rootless** — le crate `cherino-runtime` encapsule
  [libcontainer](https://github.com/containers/youki) (la bibliothèque
  sous-jacente au runtime OCI Youki) comme backend sans daemon et rootless
  pour les hôtes Linux dépourvus de Docker.
- **Une couche partagée de profils de sécurité** — profils seccomp, profil
  AppArmor FUSE, règles Landlock, politiques d'egress réseau et whitelist des
  registres, partagés par tous les backends afin que chaque runtime applique
  la même politique.

## Organisation des crates

| Crate | Description |
|-------|-------------|
| `cherino-macros` | Macro derive `Getters` utilisée par les types DTO |
| `cherino` | Trait `ContainerOps`, backend Docker, profils de sécurité, types partagés |
| `cherino-runtime` | Backend OCI Youki/libcontainer (Linux uniquement, stubs pour les autres plateformes) |

## Installation

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
```

Utilisation minimale avec le daemon Docker local :

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

## Profil AppArmor

Les conteneurs imbriqués (conteneurs créés depuis l'intérieur d'un conteneur)
nécessitent l'installation du profil AppArmor FUSE sur l'hôte, en root, une
fois par hôte :

```console
# from the cherino source tree:
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

Les hôtes portant encore l'ancien profil `celestia-plana-fuse` sont détectés
et acceptés avec un avertissement de dépréciation ; installez le nouveau nom
dès que possible.
