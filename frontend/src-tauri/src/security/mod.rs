//! Shared security boundaries for every desktop entry point.
pub(crate) mod audit;
pub(crate) mod credentials;
pub(crate) mod encryption;
pub(crate) mod erasure;
pub(crate) mod ipc;
pub mod logging;
pub(crate) mod notice;
pub(crate) mod privacy;
pub(crate) mod recovery;
pub(crate) mod storage;
pub(crate) mod transport;

#[cfg(all(test, target_os = "linux"))]
#[test]
fn optimized_glib_string_iterator_keeps_mutable_ffi_out_pointer() {
    use glib::variant::ToVariant;
    let value = ["first", "middle", "last"].to_variant();
    let mut iter = value.array_iter_str().unwrap();
    assert_eq!(iter.next(), Some("first"));
    assert_eq!(iter.next_back(), Some("last"));
    assert_eq!(iter.next(), Some("middle"));
    assert_eq!(iter.next(), None);
}
