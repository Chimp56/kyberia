#![no_main]

use kyberia_ieee80211_fuzz::exercise;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    exercise(input);
});
