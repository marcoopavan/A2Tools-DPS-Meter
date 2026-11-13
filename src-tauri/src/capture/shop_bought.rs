//! How many of each offer the character has bought in an exchange shop, as
//! the game sends it when the shop opens (`07 8C`; read 2026-11-13 on
//! Marchutan, LA 2405, from the Shugo Festival, Abyss, Season and Nightmare
//! shops):
//!
//! ```text
//! <len varint> 07 8C <u16 0> <shop u32> <count u8>
//!   count x (<offer u32> <bought u32>)
//! <now u64, unix ms>
//! ```
//!
//! Only the offers bought at least once are listed; what each sells, its
//! price and its limit are the client's own. As the game showed them that
//! day: Stigma Shards 35 of 35 in the Festival shop (990007, offer
//! 505015137), 47 of 50 in the Abyss shop (990001, offer 501015114) and 34
//! of 35 in the Nightmare shop (990003, offer 502024517).

use super::stream_processor::read_varint;

/// One shop's list: the shop and each bought offer with its count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopBought {
    pub shop: u32,
    pub offers: Vec<(u32, u32)>,
}

/// The list in a `07 8C` packet; `None` for any other, and for one whose
/// length isn't its count's or whose time isn't one.
pub fn parse(packet: &[u8]) -> Option<ShopBought> {
    let length = read_varint(packet, 0);
    if length.length <= 0 {
        return None;
    }
    let at = length.length as usize;
    if packet.get(at..at + 2)? != [0x07, 0x8C] {
        return None;
    }
    let body = &packet[at + 2..];
    if body.len() < 15 || body[..2] != [0, 0] {
        return None;
    }
    let shop = u32::from_le_bytes(body[2..6].try_into().unwrap());
    let count = body[6] as usize;
    if body.len() != 7 + 8 * count + 8 {
        return None;
    }
    let now = u64::from_le_bytes(body[7 + 8 * count..].try_into().unwrap());
    // Between 2025 and 2029.
    if !(1_735_689_600_000..1_861_920_000_000).contains(&now) {
        return None;
    }
    let offers = (0..count)
        .map(|i| {
            let o = 7 + 8 * i;
            (u32::from_le_bytes(body[o..o + 4].try_into().unwrap()), u32::from_le_bytes(body[o + 4..o + 8].try_into().unwrap()))
        })
        .collect();
    Some(ShopBought { shop, offers })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Nightmare shop's packet of 2026-11-13, as it came.
    const NIGHTMARE: [u8; 44] = [
        0x2A, 0x07, 0x8C, 0x00, 0x00, 0x33, 0x1B, 0x0F, 0x00, 0x03, 0x5F, 0x2A, 0xEC, 0x1D, 0x01, 0x00, 0x00, 0x00, 0x45, 0x49, 0xEC, 0x1D,
        0x22, 0x00, 0x00, 0x00, 0x46, 0x49, 0xEC, 0x1D, 0x32, 0x00, 0x00, 0x00, 0xBE, 0x10, 0xA3, 0xD6, 0xA1, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn a_shop_and_what_was_bought_in_it() {
        // The packet without the two bytes the capture adds after it.
        let read = parse(&NIGHTMARE[..42]).unwrap();
        assert_eq!(read.shop, 990_003);
        assert_eq!(read.offers, [(502_016_607, 1), (502_024_517, 34), (502_024_518, 50)]);
    }

    #[test]
    fn a_length_that_is_not_its_count_or_another_packet_is_not_read() {
        assert_eq!(parse(&NIGHTMARE), None);
        assert_eq!(parse(&NIGHTMARE[..41]), None);
        let mut other = NIGHTMARE[..42].to_vec();
        other[2] = 0x8D;
        assert_eq!(parse(&other), None);
        assert_eq!(parse(&[]), None);
    }
}
