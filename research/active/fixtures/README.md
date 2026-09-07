# Actual iperf3 3.20 client JSON fixtures

These four files are unchanged stdout from real one-second, single-stream, 2 Mbps IPv4 loopback TCP/UDP upload/download tests on macOS ARM64. They are not synthetic throughput. The initial proof used a trusted installed iperf3 3.20 binary and one-off servers bound only to `127.0.0.1:55291`.

Client argv was `iperf3 --client 127.0.0.1 --port 55291 --time 1 --bitrate 2000000 --parallel 1 --json --connect-timeout 1000`, adding `--reverse` for download and `--udp --length 1200` for UDP. Server argv was `iperf3 --server --bind 127.0.0.1 --port 55291 --one-off --idle-timeout 5 --json`. Both client and server exit codes were zero in all four runs. Original server outputs remain in the author's ignored `.tools` directory; their SHA-256 and independently compared summary counters are recorded in the initial runtime report.

The fixture build used the official source archive pinned in the license inventory. Its executable hash was not captured before a subsequent same-source rebuild. Do not substitute the current installed binary hash as the initial executable's hash. This evidence validates initial direct tool interoperability and subsequent parser handling, not final-wrapper live acceptance, authenticated endpoints or RF throughput.
