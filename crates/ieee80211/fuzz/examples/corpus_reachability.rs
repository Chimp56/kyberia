use kyberia_ieee80211_fuzz::{MAX_FUZZ_INPUT_BYTES, exercise};
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(
        env::args_os()
            .nth(1)
            .expect("usage: corpus_reachability CORPUS_DIRECTORY"),
    );
    let mut paths: Vec<_> = fs::read_dir(&root)
        .expect("read corpus directory")
        .map(|entry| entry.expect("read corpus entry").path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort();

    let mut canonical_documents = 0_u64;
    let mut parsed_frames = 0_u64;
    let mut parsed_fcs_frames = 0_u64;
    let mut bytes = 0_u64;
    for path in &paths {
        let input = fs::read(path).expect("read corpus input");
        assert!(
            input.len() <= MAX_FUZZ_INPUT_BYTES,
            "corpus input exceeds the reviewed harness maximum: {path:?}"
        );
        bytes = bytes
            .checked_add(u64::try_from(input.len()).expect("input length fits u64"))
            .expect("corpus byte count fits u64");
        let reached = exercise(&input);
        canonical_documents += u64::from(reached.canonical_document);
        parsed_frames += u64::from(reached.parsed_frame);
        parsed_fcs_frames += u64::from(reached.parsed_fcs_frame);
    }

    println!("files={}", paths.len());
    println!("bytes={bytes}");
    println!("canonical_documents={canonical_documents}");
    println!("parsed_frames={parsed_frames}");
    println!("parsed_fcs_frames={parsed_fcs_frames}");
}
