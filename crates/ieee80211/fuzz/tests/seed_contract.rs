use kyberia_ieee80211_fuzz::{MAX_FUZZ_INPUT_BYTES, Reachability, exercise};
use std::fs;
use std::path::PathBuf;

#[test]
fn checked_seed_corpus_reaches_every_acceptance_path() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/management_frame");
    let mut paths: Vec<_> = fs::read_dir(corpus)
        .expect("read checked seed corpus")
        .map(|entry| entry.expect("read seed entry").path())
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 6, "only the six reviewed seeds are checked in");

    let mut total = Reachability::default();
    for path in paths {
        let input = fs::read(&path).expect("read checked seed");
        assert!(
            input.len() <= MAX_FUZZ_INPUT_BYTES,
            "oversized seed: {path:?}"
        );
        let reached = exercise(&input);
        total.canonical_document |= reached.canonical_document;
        total.parsed_frame |= reached.parsed_frame;
        total.parsed_fcs_frame |= reached.parsed_fcs_frame;
    }

    assert!(
        total.canonical_document,
        "no checked canonical document reaches decode"
    );
    assert!(total.parsed_frame, "no checked raw frame reaches parse");
    assert!(
        total.parsed_fcs_frame,
        "no checked supported frame reaches FCS validation"
    );
}
