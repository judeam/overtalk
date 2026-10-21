use overtalk_protocol::{read_frame, write_frame, Message, Request, MAX_REQUEST, MAX_RESPONSE};
use serde_json::json;
use std::io::{BufReader, Cursor};

#[test]
fn seeded_fragmentation_preserves_frames_and_identifiers() {
    let mut state = 20261018u64;
    for id in 1..=256 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let request = Request {
            id,
            command: "workspace_search".into(),
            args: json!({"query":"Synthetic — नमस्ते 日本語\n".repeat(state as usize % 128)}),
        };
        let mut bytes = b"native startup line\n\n".to_vec();
        write_frame(&mut bytes, &request, MAX_REQUEST).unwrap();
        write_frame(&mut bytes, &request, MAX_REQUEST).unwrap();
        let mut reader = BufReader::with_capacity(1 + state as usize % 127, Cursor::new(bytes));
        for _ in 0..2 {
            let frame = read_frame(&mut reader, MAX_REQUEST).unwrap().unwrap();
            let decoded: Request = serde_json::from_slice(&frame).unwrap();
            decoded.validate().unwrap();
            assert_eq!(decoded.id, id);
            assert_eq!(decoded.args, request.args);
        }
        assert!(read_frame(&mut reader, MAX_REQUEST).unwrap().is_none());
    }
}

#[test]
fn malformed_envelopes_cannot_be_valid_requests() {
    for bytes in [
        r#"{"id":1,"id":2,"command":"read"}"#,
        r#"{"id":1,"command":"read","unexpected":true}"#,
        r#"{"id":-1,"command":"read"}"#,
        r#"{"id":0,"command":"read"}"#,
        r#"{"id":1,"command":"../../read"}"#,
        r#"{"id":1,"command":"read\nshutdown"}"#,
        r#"{"id":1,"command":"read","args":[]}"#,
        r#"{"id":1,"command":"read","args":"code"}"#,
        r#"[]"#,
        r#"null"#,
    ] {
        assert!(serde_json::from_str::<Request>(bytes)
            .map(|r| r.validate().is_err())
            .unwrap_or(true));
    }
}

#[test]
fn exact_frame_limits_are_enforced_without_partial_publication() {
    for limit in [0, 1, 2, 127, 4096, MAX_REQUEST, MAX_RESPONSE] {
        let mut bytes = Vec::new();
        let value = "x".repeat(limit.saturating_sub(2));
        let expected = limit >= 2;
        assert_eq!(write_frame(&mut bytes, &value, limit).is_ok(), expected);
        if expected {
            let frame = read_frame(&mut Cursor::new(&bytes), limit)
                .unwrap()
                .unwrap();
            assert_eq!(frame.len(), limit);
            assert_eq!(serde_json::from_slice::<String>(&frame).unwrap(), value);
            bytes.pop();
            assert!(read_frame(&mut Cursor::new(bytes), limit).is_err());
        } else {
            assert!(bytes.is_empty());
        }
        let response = Message::Reply {
            id: 1,
            result: json!(value),
            error: None,
        };
        let mut output = Vec::new();
        assert!(write_frame(&mut output, &response, limit).is_err());
        assert!(output.is_empty());
    }
}
