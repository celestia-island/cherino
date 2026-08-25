# Backends

Cherino traite le sandboxing des conteneurs comme un problème de
*politique-au-dessus-du-mécanisme* : la plateforme prescrit *ce qui* est
autorisé (via les profils de sécurité), et chaque backend traduit cela dans
son mécanisme natif (configurations d'hôte Docker ou hooks de la spec OCI).
Tous les backends implémentent le même trait `ContainerOps`.

| Backend | Type | Plateforme | Mécanisme | Niveau |
|---------|------|----------|-----------|------|
| **Docker** | API | Toutes | Bollard → Docker Engine HTTP API | primaire |
| **Youki** | Natif | Linux | libcontainer → conteneurs OCI rootless | repli |
| **WSLc** | CLI | Windows | shell-out vers `wslc.exe` / `container.exe` | repli |
| **Apple Container** | CLI | macOS 26+ | CLI `container` (une VM par conteneur) | repli |

## Docker (primaire)

Le backend Docker est le choix par défaut et le chemin le plus éprouvé.
`ContainerManager` se connecte au daemon Docker local via Bollard et
implémente toute la surface de `ContainerOps`, y compris exec, la copie de
fichiers, les snapshots, les volumes et la gestion des images. Il est
disponible sur toutes les plateformes derrière la feature `docker`, activée
par défaut.

```rust
use cherino::ContainerManager;

let mgr = ContainerManager::new()?; // local daemon
// or, against a specific socket:
let mgr = ContainerManager::new_with_socket("/var/run/docker.sock")?;
```

## Youki / libcontainer (repli, Linux)

Le crate `cherino-runtime` fournit `YoukiManager`, un backend OCI natif
construit sur libcontainer. Il est **rootless et sans daemon** : les
conteneurs s'exécutent sous l'utilisateur appelant, sans service en arrière-
plan, ce qui en fait la solution de repli pour les hôtes où aucun daemon
Docker n'est disponible ou autorisé.

`cherino-runtime` compile automatiquement son backend réel sous
`cfg(target_os = "linux")`. Sur toutes les autres plateformes, il compile un
stub dont les appels renvoient des erreurs « only available on Linux », de
sorte que le code multiplateforme peut en dépendre sans condition.

**Youki est un backend de repli** : il est maintenu comme une alternative
rootless, et non comme le moteur d'orchestration principal. Préférez le
backend Docker dès qu'un daemon est disponible.

## Backends CLI (repli, Windows / macOS)

Derrière la feature optionnelle `cli-backend`, cherino embarque des
adaptateurs CLI pour les runtimes de conteneurs qui n'exposent pas d'API
locale stable :

- **WSLc** sous Windows — shell-out vers `wslc.exe` / `container.exe`.
- **Apple Container** sous macOS 26+ — pilote la CLI `container`, qui exécute
  une VM légère par conteneur.

`cli-backend` est désactivée par défaut ; activez-la explicitement sur les
hôtes Windows/macOS :

```toml
[dependencies]
cherino = { version = "0.1", features = ["cli-backend"] }
```

## Architecture de runtime à deux couches

La plateforme celestia utilise deux runtimes de conteneurs distincts à des
couches différentes :

| Couche | Runtime | Utilisé par |
|--------|---------|-------------|
| **Externe** (orchestration) | Docker/Podman | health check TUI, daemon scepter, server manager |
| **Interne** (sandbox cosmos) | Youki/libcontainer | Snowflake manager, état de l'agent Neikos |

La couche externe gère les conteneurs d'infrastructure (scepter, postgres)
via l'API Docker/Podman ; ceux-ci requièrent des fonctionnalités
d'orchestration complètes — réseau, volumes persistants, health checks,
composition multi-conteneurs. La couche interne (Cosmos) s'exécute *à
l'intérieur* du conteneur scepter et utilise Youki pour créer des conteneurs
sandboxés légers, à démarrage rapide, destinés à l'exécution des agents,
chacun avec son propre profil seccomp et ses propres limites de ressources.

## Feature flags

`cherino` :

- `docker` *(par défaut)* — le backend `ContainerManager` basé sur Bollard.
- `cli-backend` — les adaptateurs CLI WSLc / Apple Container. Désactivée par
  défaut.
- `docker-tests` — tests d'intégration nécessitant un daemon Docker actif
  (désactivée par défaut ; implique `docker`).
