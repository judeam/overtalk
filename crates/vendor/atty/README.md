Overtalk's minimal MIT-licensed atty 0.2 compatibility implementation.
All terminal detection delegates to `std::io::IsTerminal` (Rust 1.70+).
It replaces the unmaintained Windows pointer implementation, addressing
RUSTSEC-2021-0145. There are no dependencies or unsafe code.
