# Release status

Overtalk 0.1.0 is a developer preview. Its packages for Linux, Windows and macOS
are on the [releases page](https://github.com/judeam/overtalk/releases), or build
it from source ([Building](BUILDING.md)).

## What has been checked

- **Linux** (Zorin OS 18.1, an Ubuntu 24.04 base, with unprivileged user
  namespaces allowed): a fresh clone builds with the documented commands, and the
  engine passes its disposable-profile workflow checks. The Rust, web-interface
  and Python test suites pass, Clippy runs with warnings denied, and the pinned
  secret scan finds nothing in the source or its history.
- **Stock Ubuntu 23.10 and later** restrict unprivileged user namespaces through
  AppArmor, which stops the bubblewrap worker sandbox. The `.deb` ships a profile
  for the worker launcher and the package workflow proves it on GitHub's stock
  Ubuntu 24.04 runner; the AppImage cannot, and says so at start-up. The `ci.yml`
  tests still lift the restriction. See [Platforms](PLATFORMS.md#ubuntu-and-apparmor).
- **Windows and macOS:** CI builds the native app, engine and worker sandbox on
  GitHub's Windows and macOS runners and passes their Rust test suites, including
  the sandbox isolation test. The package workflow builds the Linux, Windows and
  macOS packages, installs or mounts each one and starts its engine, and runs the
  bundled FFmpeg through the commands Overtalk issues (`packaging/ffmpeg/smoke.py`).
  Recording has not been tried on real Windows or Mac hardware.
- **Licences:** every package carries [NOTICE.md](../NOTICE.md) and the licence
  texts of the Rust crates compiled into it. The AppImage adds the copyright files
  of the system libraries it bundles and an offer of their source. The Windows and
  macOS packages bundle an LGPL FFmpeg that CI builds from pinned source
  (`packaging/ffmpeg/`); each release attaches its two source tarballs.

## Before a stable release

1. **Signing.** Sign in a job that builds nothing, so dependency build scripts never
   run with signing keys available. macOS builds need a Developer ID, the hardened
   runtime and notarisation; Windows builds need an Authenticode certificate. Until
   then Windows SmartScreen warns, and macOS blocks the first launch until it is
   allowed in Privacy & Security.
2. **Ubuntu 24.10 and later.** The 24.04 fix is proven in CI. Try the `.deb` on a
   stock install of the newest Ubuntu release; no hosted runner covers it.
3. **Real hardware.** Record and transcribe on a Windows and a Mac computer,
   including a screen recording; CI runners have no microphone or speakers.

Security problems go through [SECURITY.md](../SECURITY.md), not this page.
