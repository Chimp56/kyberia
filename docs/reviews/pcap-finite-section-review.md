# PCAPNG finite section-length correction review

Review date: 2026-09-07

Reviewer: `/root/pcap_review_luna`

Disposition: **REJECTED — BLOCKER**

The proposed change in the isolated `fix/pcap-finite` worktree must not be promoted. It changed the finite boundary from `section_start + SHB_size + section_length` to `section_start + section_length`.

## BLOCKER — proposed boundary uses the wrong origin

The current IETF PCAPNG draft §4.1 defines `section_length` as the length of the following section excluding the Section Header Block. Wireshark's `wiretap/pcapng.h` states the same. The correct boundary is therefore `section_start + SHB_size + section_length`, which is the unchanged implementation on `main`.

Independent little-endian and big-endian probes used a valid 88-byte stream with `section_length = 60` (`88 - 28`) and returned `Err(Malformed)` under the proposed patch. A valid empty SHB with `section_length = 0` returned `Err(Truncated)`. Compliant finite and concatenated sections would therefore fail.

The iteration-wide audit's standalone probe wrote `data.len()` including the SHB into `section_length`; the proposed fixture repeated that nonconforming encoding. Passing tests in that worktree preserved the probe error rather than proving interoperability.

## MINOR

The existing deterministic 2,048-case mutation test does not target finite section-length arithmetic or systematically mutate the length field. This remains useful follow-up fuzz coverage, but it does not make the unchanged main calculation incorrect.

## Validation

Focused/package tests, workspace tests, Clippy with `-D warnings`, and formatting all passed in review. Those passes did not cure the proposed semantic error because its fixtures used the same incorrect premise.

The reviewer rejected `PCAP-ITER4-001` as invalid. The main parser and its original test correctly interpret finite section lengths. The rejected worktree is retained for auditability and will not be merged, committed, or described as a fix.

Primary references:

- [IETF PCAPNG draft §4.1](https://datatracker.ietf.org/doc/draft-ietf-opsawg-pcapng/)
- [Wireshark `pcapng.h`](https://github.com/wireshark/wireshark/blob/master/wiretap/pcapng.h)
