pub(crate) fn short_line_hash(input: &[u8]) -> u8 {
    (xxh32(input) & 0xff) as u8
}

fn xxh32(input: &[u8]) -> u32 {
    const PRIME1: u32 = 2_654_435_761;
    const PRIME2: u32 = 2_246_822_519;
    const PRIME3: u32 = 3_266_489_917;
    const PRIME4: u32 = 668_265_263;
    const PRIME5: u32 = 374_761_393;

    let mut index = 0;
    let mut hash = if input.len() >= 16 {
        let mut v1 = PRIME1.wrapping_add(PRIME2);
        let mut v2 = PRIME2;
        let mut v3 = 0;
        let mut v4 = 0_u32.wrapping_sub(PRIME1);
        while index <= input.len() - 16 {
            v1 = xxh_round(v1, read_u32(&input[index..]));
            v2 = xxh_round(v2, read_u32(&input[index + 4..]));
            v3 = xxh_round(v3, read_u32(&input[index + 8..]));
            v4 = xxh_round(v4, read_u32(&input[index + 12..]));
            index += 16;
        }
        v1.rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18))
    } else {
        PRIME5
    };
    hash = hash.wrapping_add(input.len() as u32);
    while index + 4 <= input.len() {
        hash = hash
            .wrapping_add(read_u32(&input[index..]).wrapping_mul(PRIME3))
            .rotate_left(17)
            .wrapping_mul(PRIME4);
        index += 4;
    }
    while index < input.len() {
        hash = hash
            .wrapping_add(u32::from(input[index]).wrapping_mul(PRIME5))
            .rotate_left(11)
            .wrapping_mul(PRIME1);
        index += 1;
    }
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(PRIME2);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(PRIME3);
    hash ^ (hash >> 16)
}

fn xxh_round(value: u32, input: u32) -> u32 {
    value
        .wrapping_add(input.wrapping_mul(2_246_822_519))
        .rotate_left(13)
        .wrapping_mul(2_654_435_761)
}

fn read_u32(input: &[u8]) -> u32 {
    u32::from_le_bytes([input[0], input[1], input[2], input[3]])
}
