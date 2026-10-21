# Release status

Overtalk 0.1.0 is a developer preview. No binary has been published: build it
from source ([Building](BUILDING.md)).

## What has been checked

- **Linux** (Zorin OS 18.1, an Ubuntu 24.04 base, with unprivileged user
  namespaces allowed): a fresh clone builds with the documented commands, and the
  engine passes its disposable-profile workflow checks. The Rust, web-interface
  and Python test suites pass, Clippy runs with warnings denied, and the pinned
  secret scan finds nothing in the source or its history.
- **Stock Ubuntu 23.10 and later** restrict unprivileged user namespaces through
  AppArmor, which can stop the bubblewrap worker sandbox; see
  [Platforms](PLATFORMS.md). This has not been checked on a stock install yet.
- **Windows and macOS** have packaging and CI jobs, but those jobs have not run,
  and recording has not been tried on real Windows or Mac hardware.

## Before the first binary release

1. **Third-party notices.** Ship generated license notices for the Rust crates
   compiled into the binaries, alongside [NOTICE.md](../NOTICE.md).
2. **FFmpeg on Windows.** The Windows package bundles a GPLv3 FFmpeg build. Either
   ship it with the GPL text and its corresponding source, bundle an LGPL build,
   or leave FFmpeg to the user.
3. **AppImage libraries.** Ship the copyright files of the bundled system libraries
   and an LGPL source offer.
4. **Signing.** Sign in a job that builds nothing, so dependency build scripts never
   run with signing keys available. macOS builds need a Developer ID, the hardened
   runtime and notarisation; Windows builds need an Authenticode certificate.
5. **CI.** Run the three-platform CI and fix what it finds.

Security problems go through [SECURITY.md](../SECURITY.md), not this page.
