//! The character's own items, as the game sends them (read 2026-11-13 on
//! Marchutan, LA 2405, from one login, two transfers and the changes between).
//!
//! The whole list comes at each login and transfer, with the entry list
//! (`11 56`); a change sends the items it touched (`1B 56`: a pickup, a
//! purchase, a currency gained).
//!
//! ```text
//! <len varint> 11 56 <u16> <count varint> <u8>   count x item
//! <len varint> 1B 56 <count u8> <u16>            count x item
//!
//! item: <id of this stack u32> <u8 0> <u16> <u8 0> <item u32> <count u64>
//!       <container u8> <position u8> <3 zero bytes> <tail>
//! ```
//!
//! The tail is 10 bytes on a plain stack (an expiry, unix ms, on a season's
//! item) and longer on gear (who made it, its level, its random lines). One
//! more byte comes with about every fourth item, so the items are found by
//! their head, not by counting bytes.
//!
//! Containers: 1 the bag (currencies are items in it), 11 what is worn (the
//! Arcana cards at positions 41 to 45), 15 the Cube. 14, 16 and 18 hold the
//! rest (a spare Arcana card, the warehouse); they are passed on as they come.
//!
//! Currencies, as the game showed them beside the list: 930100031 Kinah,
//! 930100001 bound Kinah, 930100003 Abyss Points, 930100011 Shugo Festival
//! coins, 930100017 Nightmare fragments, 930100019 Oath Coins.
//!
//! An Arcana card (items 8101xxxxx to 8105xxxxx) ends its tail with its
//! attribute and its skill lines, twice over: the lines as the card came
//! (each +1) and what its upgrades added. The six cards of that day read
//! as the game showed them, levels and EXP too:
//!
//! ```text
//! <level u8> <exp u32> <9 zero bytes> <item level u32> <position u8>
//! <10 zero bytes> 03 <12 zero bytes>
//! 01 <attribute u8> 00 <value u8> <5 zero bytes>
//! <n u8> n x (<skill u32> <base u8>)  <n u8> n x (<skill u32> <added u8>)
//! ```

use super::stream_processor::read_varint;

/// An Arcana card's own numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arcana {
    pub level: u8,
    pub exp: u32,
    /// The game's id of the card's attribute (7 Justice, 8 Freedom, 9
    /// Illusion, 11 Time, 16 Destiny, as seen) and its value.
    pub attribute: u8,
    pub value: u8,
    /// Each skill line: the skill and its levels (base and added together).
    pub lines: Vec<(u32, u8)>,
}

/// One stack or piece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The game's id of this stack: a change names the stack by it.
    pub uid: u32,
    pub item: u32,
    pub count: u64,
    pub container: u8,
    pub position: u8,
    pub arcana: Option<Arcana>,
}

/// The whole list (`whole`), or the items that changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub whole: bool,
    pub items: Vec<Item>,
}

/// An item's head: what every item starts with.
const HEAD: usize = 25;
/// The shortest item: a plain stack, head and tail.
const SHORTEST: usize = 35;
/// How far before an Arcana card's attribute its level stands.
const ARCANA_LEVEL_BACK: usize = 42;

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Whether an item's head starts here. Strict on purpose: the list is read
/// by finding these, so a head must not be found inside another item.
fn head_at(body: &[u8], at: usize) -> bool {
    if at + HEAD > body.len() {
        return false;
    }
    let uid = u32_at(body, at);
    let item = u32_at(body, at + 8);
    let count = u64::from_le_bytes(body[at + 12..at + 20].try_into().unwrap());
    uid != 0
        && uid >> 24 == 0
        && body[at + 4] == 0
        && body[at + 7] == 0
        && (100_000_000..1_000_000_000).contains(&item)
        && (1..10_000_000_000).contains(&count)
        && (1..=64).contains(&body[at + 20])
        && body[at + 22..at + 25] == [0, 0, 0]
}

/// The card's numbers from its tail, found by its attribute and lines.
fn arcana(tail: &[u8]) -> Option<Arcana> {
    for at in ARCANA_LEVEL_BACK..tail.len().saturating_sub(10) {
        let (attribute, value) = (tail[at + 1], tail[at + 3]);
        let n = tail[at + 9] as usize;
        if tail[at] != 1
            || tail[at + 2] != 0
            || tail[at + 4..at + 9] != [0; 5]
            || !(1..=32).contains(&attribute)
            || !(1..=127).contains(&value)
            || !(1..=8).contains(&n)
        {
            continue;
        }
        let base = at + 10;
        let added = base + 5 * n + 1;
        if added + 5 * n > tail.len() || tail[added - 1] as usize != n {
            continue;
        }
        let mut lines = Vec::with_capacity(n);
        for i in 0..n {
            let skill = u32_at(tail, base + 5 * i);
            // The same skills in both lists, or this isn't the card's end.
            if skill == 0 || u32_at(tail, added + 5 * i) != skill {
                lines.clear();
                break;
            }
            lines.push((skill, tail[base + 5 * i + 4].saturating_add(tail[added + 5 * i + 4])));
        }
        if lines.len() != n {
            continue;
        }
        return Some(Arcana {
            level: tail[at - ARCANA_LEVEL_BACK],
            exp: u32_at(tail, at - ARCANA_LEVEL_BACK + 1),
            attribute,
            value,
            lines,
        });
    }
    None
}

/// The items of an `11 56` or `1B 56` packet; `None` for any other.
pub fn parse(packet: &[u8]) -> Option<Inventory> {
    let length = read_varint(packet, 0);
    if length.length <= 0 {
        return None;
    }
    let at = length.length as usize;
    let whole = match packet.get(at..at + 2)? {
        [0x11, 0x56] => true,
        [0x1B, 0x56] => false,
        _ => return None,
    };
    let body = &packet[at + 2..];
    let mut starts = Vec::new();
    let mut from = 0;
    while from + HEAD <= body.len() {
        if head_at(body, from) {
            starts.push(from);
            from += SHORTEST;
        } else {
            from += 1;
        }
    }
    let items = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = starts.get(i + 1).copied().unwrap_or(body.len());
            let item = u32_at(body, start + 8);
            Item {
                uid: u32_at(body, start),
                item,
                count: u64::from_le_bytes(body[start + 12..start + 20].try_into().unwrap()),
                container: body[start + 20],
                position: body[start + 21],
                arcana: (810_000_000..811_000_000)
                    .contains(&item)
                    .then(|| arcana(&body[(start + HEAD).min(end)..end]))
                    .flatten(),
            }
        })
        .collect();
    Some(Inventory { whole, items })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(uid: u32, item: u32, count: u64, container: u8, position: u8, tail: &[u8]) -> Vec<u8> {
        let mut out = uid.to_le_bytes().to_vec();
        out.extend([0, 0x67, 0x01, 0]);
        out.extend(item.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend([container, position, 0, 0, 0]);
        out.extend(tail);
        out
    }

    fn card_tail(level: u8, exp: u32, attribute: u8, value: u8, lines: &[(u32, u8)]) -> Vec<u8> {
        let mut out = vec![0; 9];
        out.push(level);
        out.extend(exp.to_le_bytes());
        out.extend([0; 9]);
        out.extend(83u32.to_le_bytes());
        out.push(41);
        out.extend([0; 10]);
        out.push(3);
        out.extend([0; 12]);
        out.extend([1, attribute, 0, value, 0, 0, 0, 0, 0]);
        out.push(lines.len() as u8);
        for (skill, _) in lines {
            out.extend(skill.to_le_bytes());
            out.push(1);
        }
        out.push(lines.len() as u8);
        for (skill, added) in lines {
            out.extend(skill.to_le_bytes());
            out.push(*added);
        }
        out.extend([0, 0, 0, 1]);
        out
    }

    fn packet(op: [u8; 2], header: &[u8], items: &[Vec<u8>]) -> Vec<u8> {
        let mut body = op.to_vec();
        body.extend(header);
        for item in items {
            body.extend(item);
        }
        // The length varint only has to be one: the parser skips it.
        let mut out = vec![0x05];
        out.extend(body);
        out
    }

    #[test]
    fn the_whole_list_with_its_currencies() {
        let items = [
            stack(0x0A586C, 930_100_031, 542_223, 1, 0, &[0; 10]),
            // A season's coin carries its expiry; every fourth item one more byte.
            stack(0x0864B5, 930_100_019, 17_880, 1, 9, &[0, 0x80, 0x99, 0xA6, 0x7D, 0xA2, 1, 0, 0, 0]),
            stack(0x08AAFA, 511_490_001, 137, 1, 28, &[0x28, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            stack(0x07E09B, 930_100_001, 13_643, 1, 1, &[0; 10]),
        ];
        let read = parse(&packet([0x11, 0x56], &[0x10, 0x02, 0x04, 0x00], &items)).unwrap();
        assert!(read.whole);
        let counts: Vec<(u32, u64, u8)> = read.items.iter().map(|i| (i.item, i.count, i.container)).collect();
        assert_eq!(
            counts,
            [(930_100_031, 542_223, 1), (930_100_019, 17_880, 1), (511_490_001, 137, 1), (930_100_001, 13_643, 1)]
        );
        assert!(read.items.iter().all(|i| i.arcana.is_none()));
    }

    #[test]
    fn an_arcana_card_with_its_lines() {
        // The Bell as the game showed it: +2, 8600 EXP, Justice 22, three
        // lines at +1 and one at +3.
        let lines = [(14_710_000, 0), (14_770_000, 0), (14_790_000, 0), (14_750_000, 2)];
        let items = [
            stack(0x0A586C, 930_100_031, 542_223, 1, 0, &[0; 10]),
            stack(0x01304E, 810_430_001, 1, 11, 44, &card_tail(2, 8_600, 7, 22, &lines)),
            stack(0x07E09B, 930_100_001, 13_643, 1, 1, &[0; 10]),
        ];
        let read = parse(&packet([0x11, 0x56], &[0x10, 0x02, 0x03, 0x00], &items)).unwrap();
        assert_eq!(read.items.len(), 3);
        let card = read.items[1].arcana.clone().unwrap();
        assert_eq!((card.level, card.exp, card.attribute, card.value), (2, 8_600, 7, 22));
        assert_eq!(card.lines, [(14_710_000, 1), (14_770_000, 1), (14_790_000, 1), (14_750_000, 3)]);
        assert_eq!((read.items[1].container, read.items[1].position), (11, 44));
        assert_eq!(read.items[2].count, 13_643);
    }

    #[test]
    fn a_change_names_the_items_it_touched() {
        let items = [stack(0x07E09B, 930_100_001, 13_643, 1, 1, &[0; 10]), stack(0x068C7F, 930_100_023, 13_992, 1, 8, &[0; 10])];
        let read = parse(&packet([0x1B, 0x56], &[0x02, 0x01, 0x00], &items)).unwrap();
        assert!(!read.whole);
        assert_eq!(read.items.iter().map(|i| (i.uid, i.count)).collect::<Vec<_>>(), [(0x07E09B, 13_643), (0x068C7F, 13_992)]);
    }

    #[test]
    fn other_packets_and_short_ones_are_not_items() {
        assert_eq!(parse(&[0x05, 0x04, 0x38, 0, 0, 0]), None);
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&[0x02, 0x11]), None);
        let empty = parse(&[0x03, 0x11, 0x56, 0x10]).unwrap();
        assert!(empty.whole && empty.items.is_empty());
    }
}
