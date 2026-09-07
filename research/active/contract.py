"""Kyberia-owned research contract, not the canonical active domain API."""
from dataclasses import asdict, dataclass
import ipaddress
import json
import math
import re
from typing import Optional

VERSION = "3.20"
MAX_JSON = 262144
MAX_EXACT_INT = 2**53 - 1  # upstream cJSON numbers pass through binary64
UDP_PAYLOAD_BYTES = 1200


class Invalid(ValueError):
    pass


def integer(value, low, high):
    if type(value) is not int or not low <= value <= high:
        raise Invalid("integer outside supported range")
    return value


def number(value, low=0, high=MAX_EXACT_INT):
    if type(value) not in (int, float) or not low <= value <= high or not math.isfinite(value):
        raise Invalid("non-finite or out-of-range number")
    return value


def text(value, limit=256):
    if not isinstance(value, str) or not value.strip() or len(value) > limit or any(ord(c) < 32 for c in value):
        raise Invalid("invalid text")
    return value


def strict_json(raw, limit=MAX_JSON):
    if not isinstance(raw, bytes) or len(raw) > limit:
        raise Invalid("JSON byte limit")
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise Invalid("duplicate JSON key")
            result[key] = value
        return result
    def parse_int(value):
        if len(value.lstrip("-")) > 16:
            raise Invalid("JSON integer overflow")
        return integer(int(value), -MAX_EXACT_INT, MAX_EXACT_INT)
    def constant(_):
        raise Invalid("non-finite JSON constant")
    try:
        result = json.loads(raw.decode("utf-8"), object_pairs_hook=pairs,
                            parse_int=parse_int, parse_constant=constant)
        count = [0]
        def visit(value, depth=0):
            count[0] += 1
            if depth > 32 or count[0] > 20000:
                raise Invalid("JSON structural limit")
            if type(value) is float:
                number(value, -MAX_EXACT_INT)
            if isinstance(value, dict):
                for child in value.values(): visit(child, depth + 1)
            elif isinstance(value, list):
                for child in value: visit(child, depth + 1)
        visit(result)
        if not isinstance(result, dict):
            raise Invalid("JSON root must be an object")
        return result
    except (UnicodeError, ValueError, RecursionError, OverflowError) as exc:
        raise Invalid(str(exc)) from exc


@dataclass(frozen=True)
class Request:
    schema_version: str
    request_id: str
    target_ip: str
    target_class: str
    port: int
    protocol: str
    direction: str
    duration_s: int
    offered_rate_bps: int  # aggregate across streams, per direction
    streams: int
    authorization: str

    def __post_init__(self):
        if self.schema_version != "1": raise Invalid("request version")
        if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", text(self.request_id, 64)):
            raise Invalid("request ID")
        text(self.target_ip, 64)
        try:
            address = ipaddress.ip_address(self.target_ip)
        except (ValueError, TypeError) as exc:
            raise Invalid("literal IP address required") from exc
        if "%" in self.target_ip or address.is_multicast or address.is_unspecified or self.target_ip == "255.255.255.255":
            raise Invalid("invalid unicast target")
        if self.target_class not in ("loopback", "lan", "remote"):
            raise Invalid("explicit topology class required")
        if address.is_loopback != (self.target_class == "loopback"):
            raise Invalid("loopback topology mismatch")
        integer(self.port, 1024, 65535)
        integer(self.duration_s, 1, 2)
        integer(self.streams, 1, 4)
        integer(self.offered_rate_bps, 1000, 10000000)
        if self.offered_rate_bps % self.streams:
            raise Invalid("rate must divide exactly across streams")
        if self.protocol not in ("tcp", "udp", "quic"):
            raise Invalid("protocol")
        if self.direction not in ("upload", "download", "bidirectional"):
            raise Invalid("direction")
        if self.authorization not in ("owned_loopback", "authenticated_agent_required"):
            raise Invalid("authorization")
        if (self.target_class == "loopback") != (self.authorization == "owned_loopback"):
            raise Invalid("authorization/topology mismatch")

    @classmethod
    def decode(cls, raw):
        try:
            return cls(**strict_json(raw, 4096))
        except TypeError as exc:
            raise Invalid("request fields") from exc

    def unsupported_reason(self):
        if self.target_class != "loopback":
            return "authenticated_non_loopback_execution_not_implemented"
        if self.target_ip != "127.0.0.1":
            return "only_ipv4_loopback_runtime_verified"
        if self.protocol == "quic" or self.direction == "bidirectional":
            return "mode_not_verified"
        if self.streams != 1:
            return "parallel_streams_not_runtime_verified"
        return None

    def argv(self, binary):
        if self.unsupported_reason(): raise Invalid(self.unsupported_reason())
        args = [binary, "--client", self.target_ip, "--port", str(self.port),
                "--time", str(self.duration_s), "--bitrate", str(self.offered_rate_bps // self.streams),
                "--parallel", str(self.streams), "--omit", "0", "--json", "--connect-timeout", "1000"]
        if self.direction == "download": args.append("--reverse")
        if self.protocol == "udp": args.extend(["--udp", "--length", str(UDP_PAYLOAD_BYTES)])
        return args


@dataclass(frozen=True)
class Transfer:
    role: str
    endpoint: str
    seconds: float
    bytes: int
    bits_per_second: float
    megabits_per_second: float
    source_sender_flag: bool
    packets: Optional[int] = None
    lost_packets: Optional[int] = None
    lost_percent: Optional[float] = None
    jitter_ms: Optional[float] = None
    retransmits: Optional[int] = None
    packet_count_semantics: Optional[str] = None
    received_datagrams: Optional[int] = None
    loss_denominator_packets: Optional[int] = None
    loss_semantics: Optional[str] = None
    source_lost_packets: Optional[int] = None
    source_lost_percent: Optional[float] = None
    source_jitter_ms: Optional[float] = None


@dataclass(frozen=True)
class Measurement:
    source_version: str
    source_system: str
    source_start_unix_ms: int
    connections: tuple
    transfers: tuple

    def json_value(self):
        return asdict(self)


def parse_result(raw, request):
    """Read only complete pinned-version client JSON; errors never become rates."""
    if request.unsupported_reason(): raise Invalid("unsupported request")
    data = strict_json(raw)
    try:
        if "error" in data: raise Invalid("iperf reported an error")
        start, end = data["start"], data["end"]
        if start["version"] != "iperf " + VERSION: raise Invalid("source version mismatch")
        system = text(start["system_info"], 1024)
        if start["connecting_to"]["host"] != request.target_ip or type(start["connecting_to"]["port"]) is not int or start["connecting_to"]["port"] != request.port:
            raise Invalid("declared target mismatch")
        test = start["test_start"]
        expected = {"protocol": request.protocol.upper(), "num_streams": request.streams,
                    "duration": request.duration_s, "reverse": int(request.direction == "download"),
                    "target_bitrate": request.offered_rate_bps // request.streams,
                    "omit": 0, "bytes": 0, "blocks": 0, "bidir": 0}
        if request.protocol == "udp": expected["blksize"] = UDP_PAYLOAD_BYTES
        for key, value in expected.items():
            if type(test[key]) is not type(value) or test[key] != value:
                raise Invalid("source/request mismatch: " + key)
        connections = start["connected"]
        if not isinstance(connections, list) or len(connections) != request.streams:
            raise Invalid("connection count mismatch")
        retained = []
        sockets = set()
        for c in connections:
            remote_host = text(c["remote_host"], 64)
            local_host = text(c["local_host"], 64)
            if ipaddress.ip_address(remote_host) != ipaddress.ip_address(request.target_ip):
                raise Invalid("target address mismatch")
            if integer(c["remote_port"], 1, 65535) != request.port:
                raise Invalid("target port mismatch")
            if not ipaddress.ip_address(local_host).is_loopback:
                raise Invalid("local address mismatch")
            integer(c["local_port"], 1, 65535)
            socket_id = integer(c["socket"], 0, MAX_EXACT_INT)
            if socket_id in sockets: raise Invalid("duplicate stream socket")
            sockets.add(socket_id)
            retained.append({key: c[key] for key in ("local_host", "local_port", "remote_host", "remote_port", "socket")})
        stamp = start["timestamp"]
        millis = integer(stamp["timemillisecs"], 0, MAX_EXACT_INT)
        secs = integer(stamp["timesecs"], 0, MAX_EXACT_INT)
        if millis // 1000 != secs: raise Invalid("source timestamp mismatch")
        if request.protocol == "udp":
            sent_packets = integer(end["sum_sent"]["packets"], 0, MAX_EXACT_INT)
            received_highest = integer(end["sum_received"]["packets"], 0, MAX_EXACT_INT)
            # A complete one-stream result cannot observe an unsent sequence.
            # Missing sender evidence is not a measured zero packet count.
            if received_highest > sent_packets:
                raise Invalid("UDP receiver sequence exceeds sender evidence")
            loss_denominator = sent_packets or received_highest
        transfers = []
        for key, role in (("sum_sent", "sender"), ("sum_received", "receiver")):
            row = end[key]
            seconds = number(row["seconds"], 0.001, 10)
            if not math.isclose(seconds, request.duration_s, rel_tol=0.25, abs_tol=0.1):
                raise Invalid("incomplete duration")
            if number(row["start"]) != 0 or not math.isclose(number(row["end"]), seconds, rel_tol=1e-6, abs_tol=1e-6):
                raise Invalid("summary window mismatch")
            size = integer(row["bytes"], 0, MAX_EXACT_INT)
            rate = number(row["bits_per_second"], 0, 1e12)
            if not math.isclose(rate, size * 8 / seconds, rel_tol=1e-5, abs_tol=0.01):
                raise Invalid("rate/bytes/duration mismatch")
            sender_flag = row["sender"]
            expected_flag = (role == "sender") if request.protocol == "udp" else request.direction == "upload"
            if type(sender_flag) is not bool or sender_flag != expected_flag:
                raise Invalid("source sender flag mismatch")
            endpoint = "client" if ((role == "sender") == (request.direction == "upload")) else "server"
            extras = {}
            if request.protocol == "udp":
                packets = integer(row["packets"], 0, MAX_EXACT_INT)
                lost = integer(row["lost_packets"], 0, max(0, packets - 1))
                percent = number(row["lost_percent"], 0, 100)
                jitter = number(row["jitter_ms"], 0, 10000)
                if not math.isclose(percent, 100 * lost / loss_denominator if loss_denominator else 0, rel_tol=1e-5, abs_tol=0.001):
                    raise Invalid("loss counters mismatch")
                extras = dict(packets=packets, source_lost_packets=lost,
                              source_lost_percent=percent, source_jitter_ms=jitter)
                if role == "sender":
                    if size != packets * UDP_PAYLOAD_BYTES or lost != 0 or percent != 0 or jitter != 0:
                        raise Invalid("UDP sender counters or placeholders mismatch")
                    extras["packet_count_semantics"] = "sent_datagrams"
                else:
                    arrivals, remainder = divmod(size, UDP_PAYLOAD_BYTES)
                    if remainder or arrivals < packets - lost or (arrivals == 0) != (packets == 0):
                        raise Invalid("UDP receiver bytes/sequence counters mismatch")
                    if arrivals < 2 and jitter != 0:
                        raise Invalid("UDP jitter without a transit-time difference")
                    extras.update(packet_count_semantics="highest_sequence_seen",
                                  received_datagrams=arrivals,
                                  loss_denominator_packets=loss_denominator,
                                  loss_semantics="iperf_sequence_gap_estimate")
                    # Source gap estimation omits trailing loss; duplicates can
                    # reduce the gap count. Never label it exact end-to-end loss.
                    if arrivals:
                        extras.update(lost_packets=lost, lost_percent=percent)
                    if arrivals >= 2: extras["jitter_ms"] = jitter
            elif "retransmits" in row:
                extras["retransmits"] = integer(row["retransmits"], 0, MAX_EXACT_INT)
            transfers.append(Transfer(role, endpoint, seconds, size, rate, rate / 1000000, sender_flag, **extras))
        return Measurement(VERSION, system, millis, tuple(retained), tuple(transfers))
    except (KeyError, TypeError, ValueError, OverflowError) as exc:
        raise Invalid(str(exc)) from exc
