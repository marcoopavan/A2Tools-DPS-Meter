//! Lê um registro de pacotes do medidor (TIMESTAMP|STREAMKEY|HEX) e escreve cada
//! quadro já remontado e descomprimido: TIMESTAMP \t STREAM \t HEX.
use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Write};

use a2tools_dps_meter_lib::capture::framing::{self, FrameKind};
use a2tools_dps_meter_lib::capture::packet_accumulator::PacketAccumulator;

fn expand(buf: &[u8], out: &mut Vec<Vec<u8>>, depth: usize) {
    if depth > 4 {
        return;
    }
    for f in framing::walk_inner(buf).frames {
        match f.kind {
            FrameKind::Packet => out.push(f.bytes(buf).to_vec()),
            FrameKind::Bundle => {
                if let Some(d) = framing::decompress_bundle(f.payload(buf)) {
                    expand(&d, out, depth + 1);
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = std::fs::File::open(&args[1]).expect("abrir entrada");
    let mut out = BufWriter::new(std::fs::File::create(&args[2]).expect("criar saída"));
    let mut streams: HashMap<String, PacketAccumulator> = HashMap::new();
    let (mut lines, mut frames) = (0usize, 0usize);
    for line in std::io::BufReader::new(input).lines() {
        let Ok(line) = line else { continue };
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '|');
        let (Some(ts), Some(key), Some(hx)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let bytes: Vec<u8> = (0..hx.len() / 2)
            .filter_map(|i| u8::from_str_radix(&hx[2 * i..2 * i + 2], 16).ok())
            .collect();
        lines += 1;
        let acc = streams.entry(key.to_string()).or_insert_with(PacketAccumulator::new);
        acc.append(&bytes);
        let buf = acc.snapshot().to_vec();
        let walk = framing::walk(&buf);
        let mut plain = Vec::new();
        for f in &walk.frames {
            match f.kind {
                FrameKind::Packet => plain.push(f.bytes(&buf).to_vec()),
                FrameKind::Bundle => {
                    if let Some(d) = framing::decompress_bundle(f.payload(&buf)) {
                        expand(&d, &mut plain, 1);
                    }
                }
            }
        }
        acc.discard_bytes(walk.consumed);
        for p in plain {
            frames += 1;
            let hex: String = p.iter().map(|b| format!("{b:02X}")).collect();
            let _ = writeln!(out, "{ts}\t{key}\t{hex}");
        }
    }
    eprintln!("{lines} linhas, {frames} quadros");
}
