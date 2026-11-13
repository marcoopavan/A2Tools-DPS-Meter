//! The pets' type understanding, as it comes at the end of the pet list the
//! game sends at each login and transfer (`00 90`; read 2026-11-13 on
//! Marchutan, LA 2405): for each pet type its three pages of nine lines, and
//! before them what the character holds of each type's essence.
//!
//! ```text
//! <len varint> 00 90 <the pets> ...
//! 06  6 x (<essence u8> <held u32>)
//! 00 <types u8>
//! types x: <type u8> <type u8> 0A <11 zero bytes> <pages u8>
//!          pages x: <page u8> <lines u8>
//!                   lines x: <slot u8> <grade u8> <stat u16> <value u64>
//! <page in use u8>
//! ```
//!
//! One byte more stands after the first line and every fourth after it: two
//! bits a line, for that line and the three after it, 3 on a locked line and
//! 2 on an open one. The five types' third pages read as the game showed
//! them that day, locks too, and the packet ends on the page they showed.
//!
//! The types seen are 2 to 6 (2 the Intellectual); essence 6 is the one every
//! type spends. A percentage's value is in hundredths (172 for 1.72%). A
//! line not yet drawn is all zeros.
//!
//! The stats named by those pages: 19 Additional Attack, 28 Damage Boost,
//! 31 Max Attack, 32 Weapon Damage Boost, 38 Critical Attack, 41 Critical
//! Hit Defense, 47 Critical Damage Tolerance, 50 Boss Attack, 52 Additional
//! Defense, 56 PvE Attack, 57 PvE Defense, 104 Additional Accuracy, 110 PvE
//! Accuracy, 128 Critical Hit, 133 Critical Hit Resist, 146 Multi-hit
//! Chance, 147 Multi-hit Resist, 193 HP, 196 HP increase, 199 MP, 379 PvE
//! Damage Boost, 443 Smite, 591 Front Critical Hit, 592 Front Critical Hit
//! Resist.

use super::stream_processor::read_varint;

/// One line of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub slot: u8,
    pub grade: u8,
    pub stat: u16,
    pub value: u64,
    pub locked: bool,
}

/// One of a type's pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub page: u8,
    pub lines: Vec<Line>,
}

/// A pet type with its pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kind {
    pub kind: u8,
    pub pages: Vec<Page>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Understanding {
    /// What is held of each essence, by its id.
    pub essences: Vec<(u8, u32)>,
    pub kinds: Vec<Kind>,
    /// The page in use, if the packet ended on it.
    pub page: Option<u8>,
}

/// A type's head: its id twice, `0A`, eleven zero bytes and its pages.
const KIND_HEAD: usize = 15;
const LINE: usize = 12;

fn kind_head_at(body: &[u8], at: usize) -> bool {
    at + KIND_HEAD <= body.len()
        && (1..=32).contains(&body[at])
        && body[at + 1] == body[at]
        && body[at + 2] == 0x0A
        && body[at + 3..at + 14] == [0; 11]
        && (1..=8).contains(&body[at + 14])
}

/// The types from `at` on and where they end, or `None` where the bytes
/// don't hold together as pages of lines.
fn kinds(body: &[u8], mut at: usize) -> Option<(Vec<Kind>, usize)> {
    let mut kinds = Vec::new();
    // The lines read so far, and the lock bits of the four being read.
    let (mut read, mut bits) = (0usize, 0u8);
    while kind_head_at(body, at) {
        let (kind, pages) = (body[at], body[at + 14]);
        at += KIND_HEAD;
        let mut out = Vec::with_capacity(pages as usize);
        for _ in 0..pages {
            let (&page, &lines) = (body.get(at)?, body.get(at + 1)?);
            if lines > 16 {
                return None;
            }
            at += 2;
            let mut list = Vec::with_capacity(lines as usize);
            for _ in 0..lines {
                let line = body.get(at..at + LINE)?;
                at += LINE;
                if read % 4 == 0 {
                    bits = *body.get(at)?;
                    at += 1;
                }
                list.push(Line {
                    slot: line[0],
                    grade: line[1],
                    stat: u16::from_le_bytes([line[2], line[3]]),
                    value: u64::from_le_bytes(line[4..].try_into().unwrap()),
                    locked: bits >> (2 * (read % 4)) & 3 == 3,
                });
                read += 1;
            }
            out.push(Page { page, lines: list });
        }
        kinds.push(Kind { kind, pages: out });
    }
    (!kinds.is_empty()).then_some((kinds, at))
}

/// The six essences that stand before the types' count, if they do.
fn essences(body: &[u8], first_kind: usize) -> Vec<(u8, u32)> {
    let Some(start) = first_kind.checked_sub(33) else { return Vec::new() };
    if body[start] != 6 {
        return Vec::new();
    }
    let list: Vec<(u8, u32)> = (0..6)
        .map(|i| {
            let at = start + 1 + 5 * i;
            (body[at], u32::from_le_bytes(body[at + 1..at + 5].try_into().unwrap()))
        })
        .collect();
    // The ids in order, or these are other bytes.
    if list.iter().enumerate().all(|(i, &(id, _))| id as usize == i + 1) {
        list
    } else {
        Vec::new()
    }
}

/// The understanding in a `00 90` packet; `None` for any other, and for a pet
/// list whose end doesn't read as types and pages to the last byte.
pub fn parse(packet: &[u8]) -> Option<Understanding> {
    let length = read_varint(packet, 0);
    if length.length <= 0 {
        return None;
    }
    let at = length.length as usize;
    if packet.get(at..at + 2)? != [0x00, 0x90] {
        return None;
    }
    let body = &packet[at + 2..];
    // The first type's head, with its first page's (`01`, a few lines) after.
    let first = (0..body.len().saturating_sub(KIND_HEAD + 1))
        .find(|&i| kind_head_at(body, i) && body[i + KIND_HEAD] == 1 && (1..=16).contains(&body[i + KIND_HEAD + 1]))?;
    let (kinds, end) = kinds(body, first)?;
    // To the end, but for the page in use: anything else was not this.
    if end + 1 < body.len() {
        return None;
    }
    Some(Understanding { essences: essences(body, first), kinds, page: body.get(end).copied() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet with `kinds` types of `pages` pages of `lines` lines; line
    /// `n` (from 0, counted across the packet) has slot `n % lines`, grade 4,
    /// stat `100 + n`, value `n`, and is locked unless `n` is in `open`.
    fn packet(kinds: u8, pages: u8, lines: u8, open: &[usize]) -> Vec<u8> {
        let mut body = vec![0x00, 0x90];
        // The pets before: an active pet and one pet, as the game lists them.
        body.extend([0xF8, 0x03, 0, 0, 0x01, 0xEA, 0x03, 0, 0, 0xEA, 0x03, 0, 0, 1, 0, 0, 0]);
        body.push(6);
        for (id, held) in [(1u8, 605u32), (2, 1456), (3, 600), (4, 221), (5, 800), (6, 23645)] {
            body.push(id);
            body.extend(held.to_le_bytes());
        }
        body.extend([0, kinds]);
        let total = kinds as usize * pages as usize * lines as usize;
        let mut n = 0usize;
        for kind in 0..kinds {
            body.extend([kind + 2, kind + 2, 0x0A]);
            body.extend([0; 11]);
            body.push(pages);
            for page in 0..pages {
                body.extend([page + 1, lines]);
                for slot in 0..lines {
                    body.extend([slot, 4]);
                    body.extend((100 + n as u16).to_le_bytes());
                    body.extend((n as u64).to_le_bytes());
                    if n % 4 == 0 {
                        let mut bits = 0u8;
                        for i in 0..4 {
                            let line = n + i;
                            let locked = line < total && !open.contains(&line);
                            bits |= (if locked { 3 } else { 2 }) << (2 * i);
                        }
                        body.push(bits);
                    }
                    n += 1;
                }
            }
        }
        body.push(3);
        let mut out = vec![0x05];
        out.extend(body);
        out
    }

    #[test]
    fn the_types_their_pages_and_their_locks() {
        let read = parse(&packet(5, 3, 9, &[1, 46, 49, 52])).unwrap();
        assert_eq!(read.essences, [(1, 605), (2, 1456), (3, 600), (4, 221), (5, 800), (6, 23645)]);
        assert_eq!(read.page, Some(3));
        assert_eq!(read.kinds.iter().map(|k| k.kind).collect::<Vec<_>>(), [2, 3, 4, 5, 6]);
        assert!(read.kinds.iter().all(|k| k.pages.len() == 3 && k.pages.iter().all(|p| p.lines.len() == 9)));
        // The second type's third page: lines 45 to 53 of the packet.
        let page = &read.kinds[1].pages[2];
        assert_eq!(page.page, 3);
        assert_eq!(page.lines.iter().map(|l| l.stat).collect::<Vec<_>>(), (145..154).collect::<Vec<u16>>());
        assert_eq!(page.lines.iter().map(|l| l.locked).collect::<Vec<_>>(), [true, false, true, true, false, true, true, false, true]);
        assert_eq!((page.lines[4].slot, page.lines[4].grade, page.lines[4].value), (4, 4, 49));
        assert!(!read.kinds[0].pages[0].lines[1].locked);
    }

    #[test]
    fn a_list_that_does_not_end_on_its_pages_is_not_read() {
        let mut cut = packet(5, 3, 9, &[]);
        cut.extend([1, 2, 3]);
        assert_eq!(parse(&cut), None);
        let mut short = packet(2, 3, 9, &[]);
        short.truncate(short.len() - 20);
        assert_eq!(parse(&short), None);
    }

    #[test]
    fn other_packets_are_not_understanding() {
        assert_eq!(parse(&[0x05, 0x11, 0x56, 0, 0]), None);
        assert_eq!(parse(&[0x03, 0x00, 0x90]), None);
        assert_eq!(parse(&[]), None);
    }
}
