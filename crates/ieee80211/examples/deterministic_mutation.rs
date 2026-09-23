use kyberia_ieee80211::{InputFraming, parse};

fn main() {
    let seeds = [
        decode_hex(include_str!("../tests/corpus/probe-request.hex")),
        decode_hex(include_str!("../tests/corpus/beacon.hex")),
        decode_hex(include_str!("../tests/corpus/probe-response.hex")),
    ];
    let mut cases = 0_u64;
    for seed in &seeds {
        for cut in 0..=seed.len() {
            let _ = parse(&seed[..cut], InputFraming::FcsAbsent);
            cases += 1;
        }
        for index in 0..seed.len() {
            for mask in [1_u8, 0x80, 0xff] {
                let mut mutated = seed.to_vec();
                mutated[index] ^= mask;
                let _ = parse(&mutated, InputFraming::FcsAbsent);
                cases += 1;
            }
        }
    }
    println!("deterministic bounded mutation cases: {cases}");
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .split_ascii_whitespace()
        .map(|part| u8::from_str_radix(part, 16).expect("checked-in corpus hex"))
        .collect()
}
