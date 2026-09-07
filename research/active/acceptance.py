"""Real iperf3 loopback proof; binds only 127.0.0.1, never external targets."""
import argparse
from dataclasses import asdict
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import socket
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from research.active.contract import Invalid, MAX_JSON, Request, strict_json
from research.active.process import execute, run


def request(protocol="tcp", direction="upload", streams=1, duration=1, port=55291):
    return Request("1", protocol + "-" + direction + "-" + str(streams), "127.0.0.1", "loopback",
                   port, protocol, direction, duration, 4000000, streams, "owned_loopback")


def server_summary(raw, req):
    """Evidence-only workaround for 3.20's identical duplicate start.target_bitrate.

    Client ingestion remains strict. No other location, count or value is permitted.
    """
    if len(raw) > MAX_JSON: raise Invalid("server JSON limit")
    class Pairs(list):
        pass
    def reduce(value, path=()):
        if isinstance(value, Pairs):
            result, seen = {}, {}
            for key, child in value:
                child = reduce(child, path + (key,))
                seen[key] = seen.get(key, 0) + 1
                if key in result:
                    if not (path == ("start",) and key == "target_bitrate" and seen[key] == 2
                            and type(child) is int and type(result[key]) is int
                            and child == result[key] == req.offered_rate_bps // req.streams):
                        raise Invalid("unexpected server duplicate")
                result[key] = child
            return result
        if isinstance(value, list): return [reduce(child, path) for child in value]
        return value
    normalized = reduce(json.loads(raw.decode("utf-8"), object_pairs_hook=Pairs))
    return strict_json(json.dumps(normalized, allow_nan=False).encode())


def loopback(binary, req, cancel_after=None, timeout_s=8):
    # Port reservation is released before exec; a bind race fails rather than using another target.
    with socket.socket() as reserve:
        reserve.bind(("127.0.0.1", 0))
        port = reserve.getsockname()[1]
    req = Request(**dict(asdict(req), port=port))
    server_cancel, client_cancel = threading.Event(), threading.Event()
    server_result, server_errors = [], []
    argv = [str(binary), "--server", "--bind", "127.0.0.1", "--port", str(port),
            "--one-off", "--idle-timeout", "5", "--server-max-duration", "2", "--json"]
    def serve():
        try:
            server_result.append(execute(argv, 10, server_cancel))
        except Exception as exc:
            # Thread exceptions must fail the acceptance run, including cleanup.
            server_errors.append(exc)
    server = threading.Thread(target=serve)
    server.start()
    timer = None
    try:
        time.sleep(0.2)
        if server_result: raise RuntimeError("server failed: " + repr(server_result[0]))
        if cancel_after is not None:
            timer = threading.Timer(cancel_after, client_cancel.set)
            timer.start()
        result = run(req, binary, client_cancel, timeout_s)
        if result["status"] == "completed": server.join(3)
        return req, result, server_result
    finally:
        if timer: timer.cancel(); timer.join()
        server_cancel.set()
        server.join(3)
        if server.is_alive(): raise RuntimeError("server not reaped")
        if server_errors: raise RuntimeError("server thread failed") from server_errors[0]
        if len(server_result) != 1: raise RuntimeError("missing server result")
        if server_result[0].cleanup_error:
            raise RuntimeError("server cleanup failed: " + server_result[0].cleanup_error)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    rows, unsupported = [], []
    for protocol in ("tcp", "udp"):
        for direction in ("upload", "download"):
            for streams in (1, 2, 4):
                candidate = request(protocol, direction, streams, 2 if streams == 4 else 1)
                if candidate.unsupported_reason():
                    unsupported.append({"request": asdict(candidate), "reason": candidate.unsupported_reason()})
                    continue
                req, result, server = loopback(binary, candidate)
                assert result["status"] == "completed", result
                assert server[0].status == "completed", server[0]
                native = server_summary(server[0].stdout, req)
                # Compare client-transferred remote counters with the server's local summary.
                remote_role = "receiver" if direction == "upload" else "sender"
                key = "sum_received" if remote_role == "receiver" else "sum_sent"
                transfer = next(t for t in result["measurement"]["transfers"] if t["role"] == remote_role)
                assert transfer["bytes"] == native["end"][key]["bytes"]
                assert transfer["bits_per_second"] > 0
                rows.append({"kind": "real_loopback", "result": result, "server_counter_agreement": "PASS",
                             "server_stdout_sha256": hashlib.sha256(server[0].stdout).hexdigest(),
                             "client_cpu_percent": native["end"]["cpu_utilization_percent"]["remote_total"],
                             "server_cpu_percent": native["end"]["cpu_utilization_percent"]["host_total"]})
    for label, options in (("cancelled", {"cancel_after": .4}), ("timeout", {"timeout_s": .3})):
        _, result, _ = loopback(binary, request(duration=2), **options)
        assert result["status"] == label and result["measurement"] is None, result
        rows.append({"kind": "real_" + label, "result": result})
    _, recovery, _ = loopback(binary, request())
    assert recovery["status"] == "completed", recovery
    rows.append({"kind": "real_recovery", "result": recovery})
    # A port held without listen cannot receive traffic or become another server.
    with socket.socket() as refused:
        refused.bind(("127.0.0.1", 0))
        result = run(request(port=refused.getsockname()[1]), binary)
    assert result["status"] == "process_error" and result["measurement"] is None, result
    rows.append({"kind": "real_connection_refused", "result": result})
    report = {"schema_version": "1", "evidence_kind": "actual_iperf3_loopback_interoperability",
              "executed_at_utc": datetime.now(timezone.utc).isoformat(), "python": platform.python_version(),
              "os": platform.platform(), "machine": platform.machine(), "checks_passed": len(rows), "runs": rows,
              "unsupported_not_executed": unsupported,
              "limitations": ["No RF, Wi-Fi, LAN, Internet, authenticated-agent or shaped-loss validation.",
                              "Counter comparison uses iperf3 endpoint summaries, not independent packet capture.",
                              "Parallel streams share a conservative 4 Mbps requested aggregate; pacing can overshoot briefly."]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True, allow_nan=False) + "\n")
    print(json.dumps({"checks_passed": len(rows), "output": str(args.output)}))


if __name__ == "__main__":
    main()
