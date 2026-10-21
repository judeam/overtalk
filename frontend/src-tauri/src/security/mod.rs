//! Shared security boundaries for every desktop entry point.
pub mod audit;
pub mod credentials;
pub mod encryption;
pub mod erasure;
pub mod ipc;
pub mod logging;
pub mod notice;
pub mod privacy;
pub mod recovery;
pub mod storage;
pub mod transport;

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
