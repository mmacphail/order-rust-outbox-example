/// Decode an Avro-encoded payload from the Confluent/Apicurio wire format.
///
/// Wire format: magic byte (0x00) + 4-byte schema ID + Avro binary string.
/// The Debezium outbox EventRouter publishes the JSONB `payload` column as an
/// Avro string, so decoding yields the raw JSON text of the order event.
pub fn decode_avro_string_payload(bytes: &[u8]) -> Option<String> {
    // Expect: magic byte 0x00 + 4-byte artifact/schema ID = 5-byte header.
    if bytes.len() < 5 || bytes[0] != 0x00 {
        return None;
    }
    let avro_bytes = &bytes[5..];

    // Avro binary string encoding: zigzag long (byte count) + UTF-8 bytes.
    let (byte_count, header_len) = read_avro_long(avro_bytes)?;
    // A negative byte count means corrupted data (valid Avro strings have non-negative length).
    if byte_count < 0 {
        return None;
    }
    let byte_count = byte_count as usize;
    let end = header_len + byte_count;
    if end > avro_bytes.len() {
        return None;
    }
    String::from_utf8(avro_bytes[header_len..end].to_vec()).ok()
}

/// Read a zigzag-encoded Avro long from the start of `bytes`.
///
/// Returns `(decoded_value, bytes_consumed)`.
pub fn read_avro_long(bytes: &[u8]) -> Option<(i64, usize)> {
    let mut n: u64 = 0;
    let mut shift = 0u32;
    let mut consumed = 0;
    loop {
        if consumed >= bytes.len() {
            return None;
        }
        let b = bytes[consumed] as u64;
        consumed += 1;
        n |= (b & 0x7F) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    // Zigzag decode: (n >> 1) XOR -(n & 1)
    let decoded = ((n >> 1) as i64) ^ -((n & 1) as i64);
    Some((decoded, consumed))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── read_avro_long ────────────────────────────────────────────────────────

    #[test]
    fn read_avro_long_empty_returns_none() {
        assert!(read_avro_long(&[]).is_none());
    }

    #[test]
    fn read_avro_long_single_byte_zero() {
        // Zigzag: encoded 0 → decoded 0, consumed 1 byte
        let (val, consumed) = read_avro_long(&[0x00]).expect("should decode 0");
        assert_eq!(val, 0);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn read_avro_long_single_byte_positive() {
        // Zigzag: encoded 2 → decoded 1 (positive zigzag: n>>1)
        let (val, consumed) = read_avro_long(&[0x02]).expect("should decode 1");
        assert_eq!(val, 1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn read_avro_long_single_byte_negative() {
        // Zigzag: encoded 1 → decoded -1
        let (val, consumed) = read_avro_long(&[0x01]).expect("should decode -1");
        assert_eq!(val, -1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn read_avro_long_multi_byte_encoding() {
        // Avro zigzag varint for value 64: encoded as 0x80 0x01
        // raw n = 128, zigzag decoded = 64
        let (val, consumed) = read_avro_long(&[0x80, 0x01]).expect("should decode 64");
        assert_eq!(val, 64);
        assert_eq!(consumed, 2);
    }

    #[test]
    fn read_avro_long_truncated_multi_byte_returns_none() {
        // High bit set means more bytes follow, but there are none
        assert!(read_avro_long(&[0x80]).is_none());
    }

    #[test]
    fn read_avro_long_ignores_trailing_bytes() {
        // Only 1 byte consumed; extra bytes are allowed (not consumed)
        let (val, consumed) = read_avro_long(&[0x04, 0xFF, 0xFF]).expect("should decode 2");
        assert_eq!(val, 2);
        assert_eq!(consumed, 1);
    }

    // ── decode_avro_string_payload ────────────────────────────────────────────

    #[test]
    fn decode_rejects_empty_input() {
        assert!(decode_avro_string_payload(&[]).is_none());
    }

    #[test]
    fn decode_rejects_input_shorter_than_five_bytes() {
        assert!(decode_avro_string_payload(&[0x00, 0x00, 0x00, 0x00]).is_none());
    }

    #[test]
    fn decode_rejects_wrong_magic_byte() {
        // Magic byte must be 0x00; anything else is invalid
        let mut bytes = vec![0x01u8, 0x00, 0x00, 0x00, 0x01];
        let text = b"hello";
        // encode length as zigzag varint: 5 * 2 = 10 → 0x0A
        bytes.push(0x0A);
        bytes.extend_from_slice(text);
        assert!(decode_avro_string_payload(&bytes).is_none());
    }

    #[test]
    fn decode_returns_none_when_avro_payload_is_empty() {
        // Valid 5-byte header but nothing after it: read_avro_long returns None,
        // triggering the ? early-return branch.
        let bytes = vec![0x00u8, 0x00, 0x00, 0x00, 0x01];
        assert!(decode_avro_string_payload(&bytes).is_none());
    }

    #[test]
    fn decode_returns_none_for_negative_byte_count() {
        // Construct a payload where the length varint decodes to -1 (zigzag 0x01)
        // Header: magic 0x00 + 4-byte schema id
        let bytes = vec![0x00u8, 0x00, 0x00, 0x00, 0x01, 0x01];
        assert!(decode_avro_string_payload(&bytes).is_none());
    }

    #[test]
    fn decode_returns_none_when_data_shorter_than_length() {
        // Claim 10 bytes but only provide 3
        // Zigzag-encode 10: 10 * 2 = 20 → 0x14
        let bytes = vec![0x00u8, 0x00, 0x00, 0x00, 0x01, 0x14, b'a', b'b', b'c'];
        assert!(decode_avro_string_payload(&bytes).is_none());
    }

    #[test]
    fn decode_valid_avro_string_payload() {
        let text = b"hello";
        // zigzag-encode length 5: 5 * 2 = 10 → 0x0A
        let mut bytes = vec![0x00u8, 0x00, 0x00, 0x00, 0x01, 0x0A];
        bytes.extend_from_slice(text);
        let result = decode_avro_string_payload(&bytes).expect("should decode 'hello'");
        assert_eq!(result, "hello");
    }

    /// Encode a non-negative integer as an Avro string length prefix.
    ///
    /// Avro encodes string lengths as zigzag-encoded longs; for non-negative `n`,
    /// the zigzag encoding is simply `n * 2`.
    fn encode_avro_length(n: usize) -> Vec<u8> {
        let mut zigzag = (n as u64) * 2; // positive zigzag encoding
        let mut out = Vec::new();
        loop {
            let byte = (zigzag & 0x7F) as u8;
            zigzag >>= 7;
            if zigzag > 0 {
                out.push(byte | 0x80);
            } else {
                out.push(byte);
                break;
            }
        }
        out
    }

    #[test]
    fn decode_valid_json_payload() {
        let json = r#"{"order_id":"abc","status":"PENDING"}"#;
        let json_bytes = json.as_bytes();
        let mut bytes = vec![0x00u8, 0x00, 0x00, 0x00, 0x01];
        bytes.extend_from_slice(&encode_avro_length(json_bytes.len()));
        bytes.extend_from_slice(json_bytes);
        let result = decode_avro_string_payload(&bytes).expect("should decode JSON payload");
        assert_eq!(result, json);
    }

    #[test]
    fn decode_valid_long_payload_uses_multi_byte_length() {
        // 100 bytes triggers the multi-byte zigzag varint branch in encode_avro_length
        // (100 * 2 = 200 > 127, requiring two continuation bytes)
        let long_str = "x".repeat(100);
        let str_bytes = long_str.as_bytes();
        let mut bytes = vec![0x00u8, 0x00, 0x00, 0x00, 0x01];
        bytes.extend_from_slice(&encode_avro_length(str_bytes.len()));
        bytes.extend_from_slice(str_bytes);
        let result = decode_avro_string_payload(&bytes).expect("should decode long payload");
        assert_eq!(result, long_str);
    }
}
