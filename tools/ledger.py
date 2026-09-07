#!/usr/bin/env python3
"""Lossless plan coverage ledger and evidence-checked execution DAG (stdlib only)."""
import argparse
import collections
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LEDGER = Path("docs/implementation/ledger.json")
DAG = Path("docs/implementation/execution-dag.json")
TRACE = Path("docs/implementation/TRACEABILITY.md")
STATUSES = {"NOT_STARTED", "IN_PROGRESS", "IMPLEMENTED", "VALIDATED",
            "BLOCKED_EXTERNAL", "DEFERRED_BY_ADR"}
DEFINITION = re.compile(r"(?:\| |#### |\- \*\*|\- )(?:\*\*)?([A-Z]+-\d{3})")
HEADING = re.compile(r"^(#{1,6}) (.+)$")
LIST = re.compile(r"^\s*(?:[-*+] |\d+\. )")


def digest(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def namespace(ancestry):
    titles = [item["title"] for item in ancestry]
    if any(t.startswith("Appendix I") for t in titles):
        return "audit"
    if any(t.startswith("18.") for t in titles):
        return "backlog"
    if any(t.startswith("20.") for t in titles):
        return "adr-proposal"
    return "catalog"


def source_role(kind, ancestry):
    titles = [item["title"] for item in ancestry]
    if kind in {"spacing", "separator", "table_separator"}:
        return "structure"
    if any(t == "Table of contents" or t.startswith("Appendix G")
           or t in {"Source index and pinned audit baseline", "Decision counts"}
           for t in titles):
        return "context"
    # Conservative: retain prose, examples, research, and prior art as reviewable
    # obligations. A source row is not automatically an independent product feature.
    return "obligation_group" if kind == "heading" else "obligation"


def assignment(original, ancestry, role):
    if role not in {"obligation", "obligation_group"}:
        return None, None
    titles = [a["title"] for a in ancestry]
    if original is None:
        for title in reversed(titles):
            match = re.match(r"([A-Z]+-\d{3})", title)
            if match:
                original = match[1]
                break
    for title in titles:
        match = re.match(r"Phase ([0-8]) —", title)
        if match:
            return "integration/release", int(match[1])
    family = original.split("-")[0] if original else None
    families = {
        "FND": ("architecture/domain", 0), "CAP": ("capture/platform", 0),
        "COL": ("capture/platform", 0), "WIFI": ("Wi-Fi semantics", 2),
        "INS": ("UI/visualization", 1), "MAP": ("spatial/survey", 3),
        "MAPB": ("spatial/survey", 1), "SUR": ("survey/positioning", 2),
        "PAS": ("Wi-Fi semantics", 2), "ACT": ("active measurement", 2),
        "ACTB": ("active measurement", 1), "ANA": ("spatial analysis", 2),
        "SPA": ("spatial analysis", 2), "PRE": ("propagation", 3),
        "PREB": ("propagation", 3), "PHY": ("Wi-Fi semantics", 3),
        "OPT": ("optimizer", 4), "OPTB": ("optimizer", 4),
        "SPE": ("spectrum", 5), "SPEB": ("spectrum", 5),
        "REQ": ("requirements", 2), "CMP": ("analysis/history", 2),
        "REP": ("reporting", 2), "UX": ("UI/visualization", 1),
        "EXT": ("ecosystem", 8), "SEC": ("security/privacy", 0),
        "OPS": ("release/operations", 0), "TST": ("QA/validation", 0),
        "OSS": ("integration", 0), "ADR": ("architecture", 0)}
    if family in families:
        return families[family]
    sections = {"6": ("product/domain", 1), "7": ("measurement/science", 2),
                "8": ("propagation", 3), "9": ("optimizer", 4),
                "10": ("architecture", 0), "11": ("storage/data", 0),
                "12": ("analysis/rendering", 1), "13": ("UI/visualization", 1),
                "14": ("architecture", 0), "15": ("security/privacy", 0),
                "16": ("QA/validation", 0), "20": ("architecture", 0),
                "23": ("QA/validation", 0)}
    for title in titles:
        match = re.match(r"(\d+)\.", title)
        if match and match[1] in sections:
            return sections[match[1]]
    return "specification/review", None


def extract(source):
    lines = source.splitlines(keepends=True)
    records, ancestry = [], []
    occurrences = collections.Counter()
    heading_occurrences = collections.Counter()
    i = 0
    while i < len(lines):
        start = i
        line = lines[i].rstrip("\r\n")
        heading = HEADING.match(line)
        if heading:
            level, title = len(heading[1]), heading[2]
            slug = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")
            heading_occurrences[slug] += 1
            hid = "heading:{}:{}".format(slug, heading_occurrences[slug])
            ancestry = [a for a in ancestry if a["level"] < level]
            ancestry.append({"id": hid, "title": title, "level": level})
            kind = "heading"
            i += 1
        elif line.startswith("```") or line.startswith("~~~"):
            fence = line[:3]
            kind = "code_or_diagram"
            i += 1
            while i < len(lines):
                end = lines[i].startswith(fence)
                i += 1
                if end:
                    break
        elif not line.strip():
            kind = "spacing"
            i += 1
            while i < len(lines) and not lines[i].strip():
                i += 1
        elif re.match(r"^\s*([-*_])(?:\s*\1){2,}\s*$", line):
            kind = "separator"
            i += 1
        elif line.startswith("|"):
            kind = "table_separator" if re.match(r"^\|[\s:|\-]+$", line) else "table_row"
            i += 1
        elif LIST.match(line):
            kind = "list_item"
            i += 1
            while (i < len(lines) and lines[i].strip() and
                   not LIST.match(lines[i]) and not HEADING.match(lines[i]) and
                   not lines[i].startswith(("|", "```", "~~~"))):
                i += 1
        else:
            kind = "paragraph"
            i += 1
            while (i < len(lines) and lines[i].strip() and
                   not LIST.match(lines[i]) and not HEADING.match(lines[i]) and
                   not lines[i].startswith(("|", "```", "~~~"))):
                i += 1
        text = "".join(lines[start:i])
        explicit = DEFINITION.match(line)
        original = explicit[1] if explicit else None
        base = ("{}:{}".format(namespace(ancestry), original) if original
                else "source:{}:{}".format(kind, digest(text)[:20]))
        occurrences[base] += 1
        rid = "{}:{}".format(base, occurrences[base])
        role = source_role(kind, ancestry)
        owner, phase = assignment(original, ancestry, role)
        records.append({
            "id": rid, "original_id": original, "kind": kind,
            "role": role,
            "source": {"path": "plan.md", "start_line": start + 1,
                       "end_line": i, "sha256": digest(text), "text": text},
            "ancestry": [dict(a) for a in ancestry],
            "status": "NOT_STARTED" if role in {"obligation", "obligation_group"} else None,
            "owner": owner, "phase": phase,
            "depends_on": [], "related_requirements": [], "acceptance_cases": [],
            "implementation": [], "validation": [], "reviews": [],
            "blocker": None, "adr": None,
        })
    heading_records = {r["ancestry"][-1]["id"]: r["id"] for r in records if r["kind"] == "heading"}
    by_id = {r["id"]: r for r in records}
    for record in records:
        parents = record["ancestry"][:-1] if record["kind"] == "heading" else record["ancestry"]
        record["parent_id"] = heading_records[parents[-1]["id"]] if parents else None
        record["children"] = []
    for record in records:
        if record["parent_id"]:
            by_id[record["parent_id"]]["children"].append(record["id"])
    explicit = [r for r in records if r["original_id"]]
    matrices = {}
    for title in ("Repository-module disposition matrix",
                  "Complete RF Atlas subsystem disposition matrix"):
        rows = [r for r in records if r["kind"] == "table_row" and
                r["ancestry"] and r["ancestry"][-1]["title"] == title]
        rows = rows[1:]  # column headings, retained as their own coverage block
        matrices[title] = {"row_count": len(rows), "record_ids": [r["id"] for r in rows],
                           "dispositions": dict(sorted(collections.Counter(
                               r["source"]["text"].split("|")[3].strip()
                               for r in rows).items()))}
    return {"schema_version": 1,
            "source": {"path": "plan.md", "sha256": digest(source), "line_count": len(lines)},
            "inventory": {"blocks": len(records),
                          "obligations": sum(r["role"] == "obligation" for r in records),
                          "obligation_groups": sum(r["role"] == "obligation_group" for r in records),
                          "coverage_only": sum(r["status"] is None for r in records),
                          "headings": sum(r["kind"] == "heading" for r in records),
                          "explicit_id_occurrences": len(explicit),
                          "unique_original_ids": len({r["original_id"] for r in explicit}),
                          "matrices": matrices},
            "records": records}


SOURCE_FIELDS = ("id", "original_id", "kind", "role", "source", "ancestry", "parent_id", "children")


def evidence_link(root, link, errors, label, sha256=None):
    if not isinstance(link, str) or not link or "\\" in link:
        errors.append(label + ": missing/invalid repository evidence link")
        return
    path = Path(link.split("#", 1)[0])
    if path.is_absolute() or ".." in path.parts or not path.parts:
        errors.append(label + ": evidence must be a repository-relative path")
        return
    resolved = (root / path).resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError:
        errors.append(label + ": evidence escapes repository")
        return
    if not resolved.is_file():
        errors.append(label + ": evidence file does not exist: " + str(path))
    elif sha256 is not None and hashlib.sha256(resolved.read_bytes()).hexdigest() != sha256:
        errors.append(label + ": stale evidence digest: " + str(path))


def evidence_record(root, item, errors, label):
    if not isinstance(item, dict):
        errors.append(label + ": malformed evidence record")
        return False
    for key in ("path", "description", "revision", "sha256"):
        if not isinstance(item.get(key), str) or not item[key]:
            errors.append(label + ": evidence missing " + key)
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", item.get("revision", "")):
        errors.append(label + ": evidence revision must be a full immutable Git or content hash")
    if not re.fullmatch(r"[0-9a-f]{64}", item.get("sha256", "")):
        errors.append(label + ": evidence requires SHA-256")
    evidence_link(root, item.get("path"), errors, label, item.get("sha256"))
    return True


def accepted_adr(root, item, errors, label):
    if not evidence_record(root, item, errors, label):
        return
    if item.get("decision_status") != "ACCEPTED" or not item.get("accepted_by"):
        errors.append(label + ": ADR must record ACCEPTED decision and approver")
    if not item.get("reason") or not item.get("preserved_product_intent"):
        errors.append(label + ": ADR deferral requires rationale and preserved intent")
    if not str(item.get("path", "")).startswith("docs/architecture/ADR/"):
        errors.append(label + ": ADR evidence must reside in docs/architecture/ADR")


def check_graph(nodes, edges, errors, label):
    visiting, visited = set(), set()
    def visit(node):
        if node in visiting:
            errors.append(label + ": dependency cycle at " + node)
            return
        if node in visited:
            return
        visiting.add(node)
        for dependency in edges.get(node, []):
            if dependency not in nodes:
                errors.append(label + ": missing dependency " + str(dependency))
            else:
                visit(dependency)
        visiting.remove(node)
        visited.add(node)
    for node in nodes:
        visit(node)


def check(ledger, dag, source, root):
    errors = []
    if not isinstance(ledger, dict) or not isinstance(dag, dict):
        return ["ledger and DAG must be JSON objects"]
    records, nodes = ledger.get("records"), dag.get("nodes")
    if not isinstance(records, list) or not isinstance(nodes, list):
        return ["ledger records and DAG nodes must be lists"]
    for label, items in (("record", records), ("DAG node", nodes)):
        for item in items:
            if not isinstance(item, dict) or not isinstance(item.get("id"), str):
                errors.append(label + " must be an object with a string ID")
                continue
            for key in ("depends_on", "related_requirements", "requirements", "acceptance_cases"):
                if key in item and (not isinstance(item[key], list) or
                                    any(not isinstance(v, str) or not v for v in item[key])):
                    errors.append(label + ": " + key + " must be a list of nonempty strings")
            for key in ("blocker", "adr"):
                if item.get(key) is not None and not isinstance(item[key], dict):
                    errors.append(label + ": " + key + " must be an object or null")
    if errors:
        return errors
    expected = extract(source)
    if ledger.get("schema_version") != 1:
        errors.append("unsupported ledger schema")
    if ledger.get("source") != expected["source"]:
        errors.append("source hash/line inventory is stale")
    if ledger.get("inventory") != expected["inventory"]:
        errors.append("source block/ID/matrix inventory differs")
    records = ledger.get("records", [])
    if len(records) != len(expected["records"]):
        errors.append("source coverage has omitted/added blocks")
    by_id = {}
    for index, record in enumerate(records):
        rid = record.get("id", "<missing>")
        if rid in by_id:
            errors.append("duplicate occurrence-qualified ID " + rid)
        by_id[rid] = record
        if index < len(expected["records"]):
            for key in SOURCE_FIELDS:
                if record.get(key) != expected["records"][index][key]:
                    errors.append("{}: stale/altered source field {}".format(rid, key))
        status = record.get("status")
        if record["role"] in {"structure", "context"}:
            if status is not None:
                errors.append(rid + ": coverage-only records cannot claim obligation status")
        elif status not in STATUSES:
            errors.append(rid + ": invalid status")
        if status == "IN_PROGRESS" and not record.get("owner"):
            errors.append(rid + ": in-progress status requires an owner")
        for field in ("implementation", "validation", "reviews"):
            evidence = record.get(field, [])
            if not isinstance(evidence, list):
                errors.append(rid + ": " + field + " must be a list")
                continue
            for item in evidence:
                if not evidence_record(root, item, errors, rid):
                    continue
                if field == "implementation" and (item.get("path") == "plan.md" or
                                                   str(item.get("path", "")).startswith("docs/implementation/")):
                    errors.append(rid + ": plan/ledger is not implementation evidence")
                if field == "validation":
                    if not all(item.get(k) for k in ("command", "result", "scope", "revision")):
                        errors.append(rid + ": validation needs command/result/scope/revision")
                    if status == "VALIDATED" and item.get("result") != "PASS":
                        errors.append(rid + ": VALIDATED contains nonpassing validation")
                if field == "reviews" and (not item.get("reviewer") or
                                            item.get("reviewer") == record.get("owner")):
                    errors.append(rid + ": review must identify an independent reviewer")
                if field == "reviews":
                    if item.get("disposition") != "APPROVED":
                        errors.append(rid + ": review disposition must be APPROVED")
                    if not isinstance(item.get("findings"), list):
                        errors.append(rid + ": review requires explicit findings list")
                    else:
                        for finding in item["findings"]:
                            if not isinstance(finding, dict) or finding.get("severity") not in {"BLOCKER", "MAJOR", "MINOR", "NIT"}:
                                errors.append(rid + ": malformed review finding")
                            elif finding.get("severity") in {"BLOCKER", "MAJOR"} and finding.get("resolution") != "RESOLVED":
                                if finding["severity"] == "MAJOR" and finding.get("resolution") == "ACCEPTED_BY_ADR":
                                    accepted_adr(root, finding.get("adr"), errors, rid)
                                else:
                                    errors.append(rid + ": unresolved " + finding["severity"] + " review finding")
        if status in {"IMPLEMENTED", "VALIDATED"}:
            if not record.get("implementation") or not record.get("acceptance_cases"):
                errors.append(rid + ": implemented status requires code and acceptance cases")
            if not record.get("owner") or not record.get("reviews"):
                errors.append(rid + ": implemented status requires owner and independent review")
        if status == "VALIDATED" and not record.get("validation"):
            errors.append(rid + ": validated status requires passing validation evidence")
        if status == "BLOCKED_EXTERNAL":
            blocker = record.get("blocker") or {}
            for key in ("dependency", "reason", "resume_procedure"):
                if not blocker.get(key):
                    errors.append(rid + ": blocker missing " + key)
            evidence_record(root, blocker.get("evidence"), errors, rid)
            for key in ("implementation", "contract_validation"):
                items = blocker.get(key)
                if not isinstance(items, list) or not items:
                    errors.append(rid + ": blocker requires surrounding " + key + " evidence")
                    continue
                for item in items:
                    if evidence_record(root, item, errors, rid) and key == "contract_validation":
                        if item.get("result") != "PASS" or not item.get("command") or item.get("scope") != "contract":
                            errors.append(rid + ": blocker contract validation must record passing command")
        if status == "DEFERRED_BY_ADR":
            accepted_adr(root, record.get("adr"), errors, rid)
    check_graph(set(by_id), {k: v.get("depends_on", []) for k, v in by_id.items()}, errors, "requirements")
    for rid, record in by_id.items():
        if record.get("status") in {"IMPLEMENTED", "VALIDATED"}:
            allowed = {"VALIDATED", "DEFERRED_BY_ADR"}
            if record["status"] == "IMPLEMENTED":
                allowed.add("IMPLEMENTED")
            pending = list(record.get("children", []))
            seen = set()
            while pending:
                child_id = pending.pop()
                if child_id in seen or child_id not in by_id:
                    continue
                seen.add(child_id)
                child = by_id[child_id]
                if child["role"] in {"obligation", "obligation_group"} and child["status"] not in allowed:
                    errors.append(rid + ": unfinished mandatory descendant " + child["id"])
                pending.extend(child.get("children", []))
        for related in record.get("related_requirements", []):
            if related not in by_id:
                errors.append(rid + ": unknown related requirement " + related)
        if record.get("status") == "VALIDATED":
            for dependency in record.get("depends_on", []):
                if dependency in by_id and by_id[dependency].get("status") != "VALIDATED":
                    errors.append(rid + ": validated requirement has unfinished dependency " + dependency)
    if dag.get("schema_version") != 1:
        errors.append("unsupported DAG schema")
    nodes = dag.get("nodes", [])
    ids = [n["id"] for n in nodes]
    if len(ids) != len(set(ids)):
        errors.append("duplicate DAG node ID")
    check_graph(set(ids), {n["id"]: n.get("depends_on", []) for n in nodes}, errors, "execution DAG")
    node_map = {n["id"]: n for n in nodes}
    for phase in range(9):
        node = node_map.get("phase-{}".format(phase))
        if not node or (phase and "phase-{}".format(phase - 1) not in node.get("depends_on", [])):
            errors.append("DAG must preserve phase delivery order: " + str(phase))
    for node in nodes:
        if not node.get("acceptance") or not node.get("owner"):
            errors.append(node["id"] + ": DAG node needs acceptance and owner")
        for rid in node.get("requirements", []):
            if rid not in by_id:
                errors.append(node["id"] + ": unknown requirement " + rid)
    return errors


def initial_dag(ledger):
    def refs(*originals):
        return [r["id"] for r in ledger["records"] if r["original_id"] in originals
                and r["id"].startswith("backlog:")]
    proofs = [
        ("contracts", [], "architecture/domain", "Unit-safe schemas, explicit unknowns, dependency tests", refs("FND-001", "FND-002", "FND-003", "OSS-012")),
        ("storage-proof", ["contracts"], "storage/data", "Real SQLite bundle/CLI, corruption/recovery and analytical-storage benchmarks", refs("FND-004", "FND-005", "FND-006", "FND-007", "FND-008")),
        ("native-capability-proof", ["contracts"], "capture/platform", "Running native probes and fixture contracts; isolate unavailable OS/hardware gates", refs("COL-001", "COL-002", "COL-004", "COL-010", "COL-011")),
        ("kismet-offline-proof", ["contracts"], "Kismet integration", "KismetDB/PCAPNG deterministic normalization and malformed-input tests", refs("OSS-002")),
        ("kismet-live-proof", ["kismet-offline-proof"], "Kismet integration", "Authentication, capabilities, source/dwell/drop context; separate live-radio gates", refs("OSS-001")),
        ("spatial-renderer-proof", ["contracts"], "spatial/UI", "Calibration and honest numerical renderer; renderer/geometry benchmark gates", refs("MAPB-001", "MAPB-002", "ANA-001", "OSS-010")),
        ("active-proof", ["contracts"], "active measurement", "Authenticated endpoint and topology-specific bounded probes", refs("ACTB-001", "ACTB-002", "ACTB-003", "ACTB-004")),
        ("sionna-proof", ["contracts"], "Sionna RT", "Pinned CPU scene/result round trip; distinct CUDA and measured gates", refs("OSS-004", "OSS-005", "OSS-006")),
        ("interchange-proof", ["contracts"], "architecture/documentation", "Neutral schema and independent deterministic round trips", refs("OSS-007")),
    ]
    nodes = [{"id": name, "kind": "prerequisite_proof", "depends_on": deps,
              "owner": owner, "acceptance": acceptance, "requirements": requirements}
             for name, deps, owner, acceptance, requirements in proofs]
    for phase in range(9):
        dependencies = ["phase-{}".format(phase - 1)] if phase else [p[0] for p in proofs]
        requirements = [r["id"] for r in ledger["records"]
                        if any(a["title"].startswith("Phase {} —".format(phase)) for a in r["ancestry"])]
        nodes.append({"id": "phase-{}".format(phase), "kind": "delivery_gate",
                      "depends_on": dependencies, "owner": "integration/release",
                      "requirements": requirements,
                      "acceptance": "All applicable phase deliverables and exit criteria independently reviewed; external runtime gates remain explicit."})
    return {"schema_version": 1,
            "semantics": "Dependencies order implementation/delivery. External validation may stay open while independent proofs and subsequent implementation proceed; it never implies a phase passed.",
            "nodes": nodes}


def render_trace(ledger):
    counts = collections.Counter(r["status"] for r in ledger["records"] if r["role"] == "obligation")
    out = ["# Plan traceability", "", "Generated by `python3 tools/ledger.py generate`; edit statuses/evidence in ledger.json.",
           "", "Every source block is retained. Counts are source coverage records, **not completed feature counts**.",
           "Structural/context rows have no implementation status. Status counts below cover leaf obligations only; group completion also checks every mandatory descendant. See LEDGER.md.", "",
           "Inventory: {} leaf obligations; {} obligation groups; {} coverage-only blocks.".format(
               ledger["inventory"]["obligations"], ledger["inventory"]["obligation_groups"], ledger["inventory"]["coverage_only"]), "",
           "Source SHA-256: `{}`".format(ledger["source"]["sha256"]), "",
           " | ".join("{}: {}".format(s, counts[s]) for s in sorted(STATUSES)), "",
           "| Source occurrence | Source / hierarchy | Role | Status | Evidence |",
           "|---|---|---|---|---|"]
    for record in ledger["records"]:
        ancestry = " / ".join(a["title"] for a in record["ancestry"])
        label = record["source"]["text"].strip().splitlines()
        title = label[0][:120] if label else "Whitespace"
        description = (ancestry + " — " + title).replace("|", "&#124;").replace("\n", " ")
        evidence = ["[{}](../../{})".format(key, item["path"])
                    for key in ("implementation", "validation", "reviews") for item in record[key]]
        if record.get("blocker"):
            evidence.append("{} ([blocker](../../{}))".format(record["blocker"]["reason"], record["blocker"]["evidence"]["path"]))
        if record.get("adr"):
            evidence.append("{} ([ADR](../../{}))".format(record["adr"]["reason"], record["adr"]["path"]))
        out.append("| `{}` | [L{}](../../plan.md#L{}) {} | {} | {} | {} |".format(
            record["id"], record["source"]["start_line"], record["source"]["start_line"], description,
            record["role"], record["status"] or "COVERAGE_ONLY", ", ".join(evidence).replace("|", "&#124;")))
    return "\n".join(out) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("generate", "check"))
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    root = args.root.resolve()
    source = (root / "plan.md").read_text(encoding="utf-8")
    if args.command == "generate":
        ledger = extract(source)
        if (root / LEDGER).exists():
            old = read_json(root / LEDGER)
            if old.get("source") != ledger["source"]:
                parser.error("plan changed: reconcile source/status evidence explicitly before regeneration")
            updates = {r["id"]: r for r in old["records"]}
            for record in ledger["records"]:
                for key, value in updates.get(record["id"], {}).items():
                    if key not in SOURCE_FIELDS:
                        record[key] = value
        dag = read_json(root / DAG) if (root / DAG).exists() else initial_dag(ledger)
        errors = check(ledger, dag, source, root)
        if errors:
            print("\n".join(errors), file=sys.stderr)
            return 1
        write_json(root / LEDGER, ledger)
        write_json(root / DAG, dag)
        (root / TRACE).write_text(render_trace(ledger), encoding="utf-8")
    else:
        ledger, dag = read_json(root / LEDGER), read_json(root / DAG)
        errors = check(ledger, dag, source, root)
        if not (root / TRACE).is_file() or (root / TRACE).read_text(encoding="utf-8") != render_trace(ledger):
            errors.append("TRACEABILITY.md is stale; regenerate after evidence updates")
        if errors:
            print("\n".join(errors), file=sys.stderr)
            return 1
    print("PASS: {} source blocks; {} explicit ID occurrences; {} headings".format(
        ledger["inventory"]["blocks"], ledger["inventory"]["explicit_id_occurrences"], ledger["inventory"]["headings"]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
