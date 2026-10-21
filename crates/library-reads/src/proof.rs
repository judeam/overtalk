//! Before a client sends its token, the app proves it holds the same token: it answers
//! a fresh nonce with an HMAC under that token. A program that took the API port while
//! Overtalk was closed cannot answer, so the token never reaches it. The MAC covers the
//! port the app listens on, so a program on another port cannot relay the app's answer.
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

const CONTEXT: &[u8] = b"overtalk-local-api-proof\n";

fn mac(token: &str, port: u16, nonce: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC takes any key");
    mac.update(CONTEXT);
    mac.update(format!("{port}\n").as_bytes());
    mac.update(nonce.as_bytes());
    mac
}

/// A nonce is 32 to 128 lowercase hex characters.
pub fn valid_nonce(nonce: &str) -> bool {
    (32..=128).contains(&nonce.len())
        && nonce
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The app's answer to `nonce` on the port it listens on, as lowercase hex.
pub fn proof(token: &str, port: u16, nonce: &str) -> String {
    mac(token, port, nonce)
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Whether `answer` proves the token for `nonce` on `port`, compared in constant time.
pub fn verify(token: &str, port: u16, nonce: &str, answer: &str) -> bool {
    let bytes: Option<Vec<u8>> = (answer.len() == 64)
        .then(|| {
            (0..answer.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(answer.get(i..i + 2)?, 16).ok())
                .collect()
        })
        .flatten();
    bytes.is_some_and(|bytes| mac(token, port, nonce).verify_slice(&bytes).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_proves_only_the_same_token_port_and_nonce() {
        let (token, nonce) = ("t".repeat(64), "0123456789abcdef0123456789abcdef");
        let answer = proof(&token, 8421, nonce);
        // Computed independently with Python's hmac module.
        assert_eq!(
            answer,
            "1356ca7085ed71c3406b2eeacd81f6c7acf5770c55e15ebb017baf3df734b259"
        );
        assert!(verify(&token, 8421, nonce, &answer));
        assert!(!verify(&"u".repeat(64), 8421, nonce, &answer));
        assert!(!verify(&token, 8422, nonce, &answer));
        assert!(!verify(
            &token,
            8421,
            "0123456789abcdef0123456789abcdee",
            &answer
        ));
        assert!(!verify(&token, 8421, nonce, &answer[..62]));
        assert!(!verify(&token, 8421, nonce, "zz"));
        assert!(valid_nonce(nonce) && !valid_nonce("short") && !valid_nonce(&"G".repeat(32)));
    }
}
