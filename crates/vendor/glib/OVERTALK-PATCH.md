# GLib 0.18 compatibility security patch

Upstream: glib 0.18.5 from crates.io, registry checksum
`233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`.

Security change: backport gtk-rs/gtk-rs-core#1343 to
`VariantStrIter::impl_get`, passing a mutable out-pointer to `g_variant_get_child`.
https://github.com/gtk-rs/gtk-rs-core/pull/1343
Addresses RUSTSEC-2024-0429 while GTK3's API still pins GLib 0.18.
The upstream variant iterator tests exercise it in optimized builds.

Compiler compatibility cleanup (18 October 2026): make 31 existing elided
lifetimes explicit in borrowed return types and remove redundant parentheses
around `&[Value]` types. These are Rust 1.94 compiler suggestions; they do not
change ownership, ABI, iterator behavior or the security backport above.

All upstream MIT licensing/copyright files are retained. Remove this fork when
the GTK/Tauri dependency can use an upstream fixed release. This is a maintained
local patch, not an upstream 0.18.5 security release.
