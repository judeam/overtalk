//! Compatibility API for transitive users of atty 0.2, backed by Rust's safe
//! standard-library terminal detection. No custom Windows pointer code.
use std::io::IsTerminal;
#[derive(Clone, Copy, Debug)]
pub enum Stream { Stdout, Stderr, Stdin }
pub fn is(stream: Stream) -> bool {
    match stream {
        Stream::Stdout => std::io::stdout().is_terminal(),
        Stream::Stderr => std::io::stderr().is_terminal(),
        Stream::Stdin => std::io::stdin().is_terminal(),
    }
}
pub fn isnt(stream: Stream) -> bool { !is(stream) }
