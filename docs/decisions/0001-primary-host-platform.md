# ADR 0001: Use Gentoo Linux on the Primary Host

- **Status:** Accepted
- **Date:** 2026-07-29

## Context

Strife v1 needs a defined primary environment so dependency availability, development instructions, and resource constraints are concrete. The available home server is a Raspberry Pi 5 with 4 GB of RAM, while the project must remain buildable for x86-64 as well.

## Decision

The primary v1 host is Gentoo Linux on ARM64. Development and validation will target the Raspberry Pi 5 first, while Rust code, frontend assets, external-tool selection, and future container definitions must also support x86-64.

Strife must operate without internet access after its dependencies are installed. Worker concurrency and child-process limits must be tuned for the 4 GB primary host.

## Alternatives Considered

- Raspberry Pi OS or another Debian-family distribution
- Ubuntu Server
- Supporting several host distributions equally in v1

## Consequences

- Gentoo package names, service conventions, and ARM64 availability must be documented where host setup matters.
- The project cannot assume Debian packages or paths.
- x86-64 compatibility remains a build requirement, but Gentoo ARM64 is the reference runtime.

