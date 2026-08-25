# Sécurité

Cherino applique une défense en profondeur pour les conteneurs sandboxés
grâce à une couche partagée de profils de sécurité. Les mêmes types de
politique sont consommés par chaque backend, de sorte que les runtimes Docker
et Youki appliquent des restrictions identiques.

## Profils de sécurité

`ContainerSecurity` regroupe la politique par charge de travail :

- `cap_drop` / `cap_add` — ensembles de capabilities Linux ;
- `security_opt` — options de sécurité Docker (p. ex. `no-new-privileges:true`) ;
- `egress_policy` — la politique d'egress réseau (voir ci-dessous).

Des profils prêts à l'emploi résident dans `cherino::security_profile` :
`postgres()`, `scepter()`, `scepter_readonly()` et `cosmos()`. Ils encodent
les valeurs par défaut durcies de la plateforme — par exemple, `scepter()`
retire toutes (`ALL`) les capabilities et ne réajoute que l'ensemble minimal
nécessaire à ses sous-conteneurs.

Avec la feature `docker`, `cherino::apply_to_host_config` applique un
`ContainerSecurity` à un `HostConfig` Bollard, en traduisant la politique en
paramètres natifs de Docker (y compris les règles d'egress).

## seccomp

`SeccompProfile` / `SeccompProfileData` décrivent des profils de filtrage des
appels système, et `build_security_opts` rend un profil sous forme d'entrées
`security_opt` pour la configuration du conteneur. Le filtrage seccomp est la
première ligne de défense : il restreint les appels système qu'une charge de
travail sandboxée peut effectuer tout court.

## AppArmor

Les conteneurs imbriqués doivent monter des systèmes de fichiers FUSE, ce que
la politique AppArmor par défaut bloque. Cherino embarque un profil dédié,
`celestia-cherino-fuse`
(`crates/cherino/src/apparmor/celestia-cherino-fuse`), à installer une seule
fois sur l'hôte, en root :

```console
install -m 0644 crates/cherino/src/apparmor/celestia-cherino-fuse \
        /etc/apparmor.d/celestia-cherino-fuse
apparmor_parser -r /etc/apparmor.d/celestia-cherino-fuse
```

À l'exécution, cherino détecte quel profil est installé
(`installed_profile_name`) et l'attache via `fuse_security_opts`. Les hôtes
portant encore l'ancien profil `celestia-plana-fuse` sont acceptés avec un
avertissement de dépréciation.

La porte de sortie `CHERINO_APPARMOR_UNCONFINED` (ancien nom :
`PLANA_APPARMOR_UNCONFINED`) permet de désactiver le confinement AppArmor sur
les hôtes où aucun profil ne peut être installé — chaque utilisation émet un
`tracing::warn!`.

## Landlock

`LandlockRules` exprime des règles d'accès au système de fichiers appliquées
via Linux Landlock, restreignant les chemins qu'un processus sandboxé peut
toucher, indépendamment des frontières des conteneurs.

## Contrôle de l'egress

`EgressPolicy` contrôle les accès réseau sortants avec trois modes
(`EgressMode`) : `DenyAll` (par défaut), `AllowAll` et `Whitelist`. Les
politiques se construisent de manière fluide :

```rust
use cherino::EgressPolicy;

let policy = EgressPolicy::whitelist()
    .allow_host("crates.io")
    .allow_host_with_port("github.com", 443)
    .allow_network("192.0.2.0/24")
    .with_dns_server("192.0.2.53");
```

`EgressPolicy::deny_all()` bloque tout egress ; `entelecheia_default()`
renvoie la liste d'autorisation standard de la plateforme. Sous Docker, la
politique est réalisée par épinglage DNS et entrées `extra_hosts`, afin que
seuls les noms en whitelist soient résolus vers de véritables adresses.

## Whitelist des registres

`RegistryWhitelist` (avec `RegistryEntry`) restreint les registres d'images
depuis lesquels les conteneurs peuvent être tirés. Les whitelists peuvent
être chargées depuis un fichier (`RegistryWhitelist::load`), analysées depuis
du texte (`RegistryWhitelist::parse`) ou résolues depuis un workspace
(`resolve_from_workspace`).

## Fonctionnement rootless

Le backend `cherino-runtime` (Youki/libcontainer) exécute les conteneurs en
mode rootless et sans daemon : aucun daemon privilégié ne détient l'état des
conteneurs, et les charges de travail sandboxées s'exécutent sous
l'utilisateur appelant — réduisant ainsi la surface d'attaque privilégiée sur
les hôtes où il est utilisé.

## Marque et compatibilité

`cherino` a été extrait du workspace `plana`. Les anciens noms sont toujours
lus pour compatibilité, chacun émettant un `tracing::warn!` :

| Ancien nom (plana / entelecheia) | Nouveau nom (cherino) |
|----------------------------------|-----------------------|
| Profil AppArmor `celestia-plana-fuse` | `celestia-cherino-fuse` |
| env `PLANA_APPARMOR_UNCONFINED` | env `CHERINO_APPARMOR_UNCONFINED` |
| env `ENTELECHEIA_RUN_DIR` | env `CHERINO_RUN_DIR` |
| répertoire d'exécution `/tmp/entelecheia/youki` | répertoire d'exécution `/tmp/cherino/youki` |

Les surcharges génériques (`CONTAINER_RUN_DIR`, `CONTAINER_ROOTFS_URL`,
`CONTAINER_NETWORK`) gardent la priorité sur tous les noms de marque.
