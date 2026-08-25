# Exemples

Tous les exemples compilent contre les API réelles de `cherino` /
`cherino-runtime`. Ajoutez d'abord les crates :

```toml
# Cargo.toml
[dependencies]
cherino = "0.1"
cherino-runtime = "0.1" # optional: rootless OCI backend (Linux)
anyhow = "1"
tokio = { version = "1", features = ["full"] }
```

## Lister les conteneurs (backend Docker)

Le démarrage rapide minimal : se connecter au daemon Docker local et lister
les conteneurs en cours d'exécution via le trait `ContainerOps`.

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

## Créer, démarrer, exec, supprimer

Le cycle de vie complet via `ContainerOps`. `ContainerCreateParams::simple`
remplit chaque champ avec une valeur par défaut sûre ; surchargez ce dont
vous avez besoin.

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

## Conteneurs OCI rootless (backend Youki)

`YoukiManager`, fourni par `cherino-runtime`, implémente le même trait
`ContainerOps`, en mode rootless et sans daemon, sous Linux. La même séquence
d'appels fonctionne — seule la construction diffère.

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

Sur les plateformes non-Linux, `cherino-runtime` compile un stub avec le même
type et la même implémentation de trait, dont les appels renvoient des
erreurs « only available on Linux », de sorte que le code multiplateforme
compile sans modification.

## Appliquer un profil de sécurité

Les charges de travail de la plateforme utilisent les profils prêts à
l'emploi de `cherino::security_profile`. Chaque `ContainerSecurity`
correspond un-à-un aux champs de `ContainerCreateParams` :

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

## Politique d'egress personnalisée

Construisez une whitelist d'egress de manière fluide et attachez-la aux
paramètres du conteneur :

```rust
use cherino::{ContainerCreateParams, EgressPolicy};

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .with_dns_server("192.0.2.53");

let mut params = ContainerCreateParams::simple("net-restricted", "alpine:latest");
params.egress_policy = Some(policy);
```

## Une note sur le Snowflake manager

Le Snowflake manager — l'orchestrateur de sandbox par workspace qui exécute
les charges de travail des agents — ne fait **pas partie de cherino** : il
réside en aval, dans le workspace entelecheia, en tant que consommateur de la
couche runtime interne (Cosmos). Il sélectionne son backend via
`ContainerOps`, avec Youki/libcontainer par défaut
(`COSMOS_CONTAINER_RUNTIME=youki`), et superpose sa propre politique d'egress
par workspace à `cherino::security_profile::cosmos()`. Le modèle à reproduire
est celui ci-dessus : dépendre du trait `ContainerOps`, construire le backend
pris en charge par l'hôte, et appliquer les profils de sécurité partagés.
