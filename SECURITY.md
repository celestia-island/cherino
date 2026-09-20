# Security Policy

## Reporting a vulnerability

**Do not open a public issue for a security vulnerability.**

Report it privately through GitHub Security Advisories:

> https://github.com/celestia-island/cherino/security/advisories/new

If advisories are unavailable to you, email security@celestia.world with a clear description
and reproduction steps. Include (1) the affected component and version, (2) the attack
vector and impact, (3) reproduction steps, (4) suggested mitigations, and whether you
intend to publish.

## Scope

In scope, across the organization:

- authentication bypass, JWT/OAuth weaknesses, session handling flaws;
- API key or credential disclosure, or improper storage;
- authorization and RBAC enforcement gaps, cross-tenant or cross-workspace access;
- injection (SQL, command, SSRF, XSS), insecure deserialization, path traversal;
- anything that lets a network peer reach a hardware command path, a safety interlock,
  or a write whitelist without an explicit operator grant;
- billing-ledger integrity: minting, double spending, or unbalanced entries;
- supply chain: build scripts, CI workflows, dependency confusion, artefact substitution.

Out of scope:

- vulnerabilities in upstream dependencies that are not exploitable through this project;
- self-hosted deployments configured against documented guidance;
- denial of service against public model-provider endpoints.

## Response targets

| Stage | Target |
|---|---|
| Agent acknowledgment | 10 minutes |
| Human acknowledgment | 1 calendar day |
| Initial assessment | 3 calendar days |
| Fix or mitigation | 30 calendar days, severity-dependent |

## What we will not accept

Bulk, machine-generated reports that were never verified against a running build, and
reports that consist only of scanner output. They will be closed without review. This
is a deliberate policy against report flooding, not a dismissal of genuine findings.

## Supported versions

Only the current default branch (`master`; a few repositories use `main`) receives
security fixes. The retired `dev` branch model is not supported — do not report against
it and do not open pull requests against it.

---
*Canonical file maintained in the organization metadata repository; changes apply to
every repository that adopts it.*

<!-- repository-specific notes below this line -->
## What this repository builds

`cherino` builds and manages sandboxed containers. Security-relevant surfaces,
beyond the organization-wide list:

- seccomp / AppArmor / Landlock profile generation (`cherino::seccomp`,
  `cherino::apparmor`, `cherino::landlock`);
- egress and registry-whitelist policy enforcement (`cherino::egress`,
  `cherino::registry_whitelist`);
- the rootless OCI backend (`cherino-runtime`).

The development-only escape hatches (`CHERINO_APPARMOR_UNCONFINED`,
`DISABLE_SECCOMP`) must never be set in production. Reports about their misuse in
a deployment are out of scope for this repository — they are deployment
configuration errors.

## Local verification before opening a pull request

```console
just check        # cargo check --workspace --all-features
just test         # cargo test --workspace
just lint         # cargo fmt --all --check + clippy -D warnings
```

## Code style

- Rust edition 2024, MSRV 1.91, `rustfmt` with `max_width = 100`.
- Clippy must pass with `-D warnings` across `--all-features --tests`.
- Brand-sensitive identifiers (AppArmor profile names, environment overrides)
  carry legacy compatibility paths — extend them, never silently rename them.
- Never commit real credentials, tokens, or internal network addresses. Use
  placeholders (`CHANGE_ME`, `test-password`, RFC 5737 `192.0.2.x`) in examples
  and tests.
