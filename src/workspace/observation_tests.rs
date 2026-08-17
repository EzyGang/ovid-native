use crate::workspace::line_hash::short_line_hash;

#[test]
fn short_line_hash_matches_xxh32_reference_low_bytes() {
    assert_eq!(short_line_hash(b""), 0x05);
    assert_eq!(short_line_hash(b"a"), 0x56);
    assert_eq!(short_line_hash(b"hello"), 0xf9);
}
