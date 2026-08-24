# Security policy

## Reporting a vulnerability

This repository is part of the celestia-island organization. Report security
issues privately to `sysl.contact@celestia.world`. Please include reproduction
steps and affected commit hashes. Do not open public issues for suspected
vulnerabilities.

## Scope

`cherino` builds and manages sandboxed containers. Security-relevant surfaces:

- seccomp / AppArmor / Landlock profile generation (`cherino::seccomp`,
  `cherino::apparmor`, `cherino::landlock`)
- egress and registry-whitelist policy enforcement (`cherino::egress`,
  `cherino::registry_whitelist`)
- the rootless OCI backend (`cherino-runtime`)

The dev-only escape hatches (`CHERINO_APPARMOR_UNCONFINED`,
`DISABLE_SECCOMP`) must never be set in production; reports about their
misuse in deployments are out of scope.

## AI-generated code notice

The code is substantially AI-generated (see [LICENSE](LICENSE) model
disclosure). Review changes with that failure profile in mind.
