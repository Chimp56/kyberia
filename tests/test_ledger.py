"""Adversarial source/evidence tests: no production capability is certified here."""
import copy
import importlib.util
import hashlib
from contextlib import contextmanager
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ledger", ROOT / "tools/ledger.py")
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


@contextmanager
def retained_temp_directory():
    # User policy forbids recursive deletion; retain these tiny test fixtures.
    yield tempfile.mkdtemp(prefix="kyberia-ledger-test-")


class LedgerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = (ROOT / "plan.md").read_text(encoding="utf-8")
        cls.baseline = ledger.extract(cls.source)
        cls.dag = ledger.initial_dag(cls.baseline)

    def setUp(self):
        self.data = copy.deepcopy(self.baseline)
        self.graph = copy.deepcopy(self.dag)

    def check(self, source=None, root=ROOT):
        return ledger.check(self.data, self.graph, source or self.source, root)

    def record(self, rid="backlog:FND-001:1"):
        return next(r for r in self.data["records"] if r["id"] == rid)

    def evidence(self, path="tools/ledger.py", **extra):
        checksum = hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
        return dict({"path": path, "description": "test-only evidence",
                     "sha256": checksum, "revision": checksum}, **extra)

    def test_complete_source_reconstructs_byte_for_byte(self):
        self.assertEqual("".join(r["source"]["text"] for r in self.data["records"]), self.source)
        self.assertFalse(self.check())
        next_line = 1
        for record in self.data["records"]:
            self.assertEqual(record["source"]["start_line"], next_line)
            next_line = record["source"]["end_line"] + 1
        self.assertEqual(next_line, 6202)

    def test_all_explicit_occurrences_and_headings(self):
        inventory = self.data["inventory"]
        self.assertEqual(inventory["explicit_id_occurrences"], 438)
        self.assertEqual(inventory["unique_original_ids"], 341)
        self.assertEqual(inventory["headings"], 446)
        self.assertEqual(len({r["id"] for r in self.data["records"]}), len(self.data["records"]))

    def test_conflicting_original_ids_are_not_merged(self):
        a, b = self.record(), self.record("audit:FND-001:1")
        self.assertEqual(a["original_id"], b["original_id"])
        self.assertNotEqual(a["source"]["text"], b["source"]["text"])
        self.assertIn("domain units", a["source"]["text"])
        self.assertIn("Desktop shell", b["source"]["text"])
        adr = [r for r in self.data["records"] if r["original_id"] == "ADR-012"]
        self.assertEqual([r["id"] for r in adr], ["adr-proposal:ADR-012:1", "adr-proposal:ADR-012:2"])

    def test_matrix_dispositions_match_complete_audit(self):
        matrices = self.data["inventory"]["matrices"]
        repo = matrices["Repository-module disposition matrix"]
        product = matrices["Complete RF Atlas subsystem disposition matrix"]
        self.assertEqual(repo["row_count"], 80)
        self.assertEqual(product["row_count"], 172)
        self.assertEqual(repo["dispositions"], {"ADOPT": 8, "INTEGRATE": 13, "CONTRIBUTE": 11,
                                                "REIMPLEMENT": 7, "REFERENCE-ONLY": 41})
        self.assertEqual(product["dispositions"], {"ADOPT": 3, "INTEGRATE": 7, "CONTRIBUTE": 4,
                                                   "REIMPLEMENT": 154, "REFERENCE-ONLY": 4})

    def test_omitted_block_rejected(self):
        self.data["records"].pop(500)
        self.assertTrue(any("coverage" in e for e in self.check()))

    def test_new_source_requirement_rejected(self):
        self.assertTrue(any("stale" in e for e in self.check(self.source + "\n- Must survive new requirement.\n")))

    def test_edited_text_or_hash_rejected(self):
        record = self.record()
        record["source"]["text"] = "Pretend the units requirement disappeared."
        record["source"]["sha256"] = ledger.digest(record["source"]["text"])
        self.assertTrue(any("source field" in e for e in self.check()))

    def test_false_inventory_rejected(self):
        self.data["inventory"]["matrices"]["Complete RF Atlas subsystem disposition matrix"]["row_count"] = 171
        self.assertTrue(any("inventory" in e for e in self.check()))

    def test_bad_ancestry_rejected(self):
        self.record()["ancestry"] = []
        self.assertTrue(any("ancestry" in e for e in self.check()))

    def test_duplicate_qualified_id_rejected(self):
        self.record("audit:FND-001:1")["id"] = "backlog:FND-001:1"
        self.assertTrue(any("duplicate occurrence" in e for e in self.check()))

    def test_illegal_status_rejected(self):
        self.record()["status"] = "DONE"
        self.assertTrue(any("invalid status" in e for e in self.check()))

    def test_placeholder_completion_rejected(self):
        self.record()["status"] = "VALIDATED"
        errors = self.check()
        self.assertTrue(any("requires code" in e for e in errors))
        self.assertTrue(any("passing validation" in e for e in errors))
        self.assertTrue(any("independent review" in e for e in errors))

    def test_missing_or_absolute_evidence_rejected(self):
        self.record()["implementation"] = [{"path": "absent.rs", "description": "absent"},
                                            {"path": "/etc/passwd", "description": "outside"},
                                            {"path": "../outside", "description": "traversal"}]
        errors = self.check()
        self.assertTrue(any("does not exist" in e for e in errors))
        self.assertTrue(any("repository-relative" in e for e in errors))

    def test_symlink_evidence_escape_rejected(self):
        with retained_temp_directory() as temp:
            root = Path(temp)
            (root / "outside").symlink_to(ROOT / "plan.md")
            self.record()["implementation"] = [{"path": "outside", "description": "escape"}]
            self.assertTrue(any("escapes" in e for e in self.check(root=root)))

    def validated(self):
        record = self.record()
        record.update({"status": "VALIDATED", "owner": "author", "acceptance_cases": ["unit mismatch is rejected"],
                       "implementation": [self.evidence()],
                       "validation": [self.evidence(command="test-command", result="PASS", scope="unit")],
                       "reviews": [self.evidence(reviewer="reviewer", disposition="APPROVED", findings=[])]})
        return record

    def test_independent_review_and_passing_result_required(self):
        record = self.validated()
        self.assertFalse(self.check())
        record["reviews"][0]["reviewer"] = "author"
        record["validation"][0]["result"] = "FAIL"
        errors = self.check()
        self.assertTrue(any("independent reviewer" in e for e in errors))
        self.assertTrue(any("nonpassing" in e for e in errors))

    def test_blocker_requires_specific_evidence_and_resume(self):
        record = self.record()
        record["status"] = "BLOCKED_EXTERNAL"
        self.assertTrue(any("blocker missing" in e for e in self.check()))
        record["blocker"] = {"dependency": "test device", "reason": "physically absent",
                             "resume_procedure": "run named device gate", "evidence": self.evidence(),
                             "implementation": [self.evidence()],
                             "contract_validation": [self.evidence(command="contract-test", result="PASS", scope="contract")]}
        self.assertFalse(self.check())

    def test_adr_requires_preserved_product_intent(self):
        record = self.record()
        record["status"] = "DEFERRED_BY_ADR"
        record["adr"] = self.evidence(reason="test")
        self.assertTrue(any("preserved intent" in e for e in self.check()))
        with retained_temp_directory() as temp:
            root = Path(temp)
            path = root / "docs/architecture/ADR/0001-example.md"
            path.parent.mkdir(parents=True)
            path.write_text("Accepted decision", encoding="utf-8")
            checksum = hashlib.sha256(path.read_bytes()).hexdigest()
            record["adr"] = {"path": "docs/architecture/ADR/0001-example.md", "description": "test",
                             "revision": checksum, "sha256": checksum, "reason": "test",
                             "preserved_product_intent": "named replacement evidence",
                             "decision_status": "ACCEPTED", "accepted_by": "code-owner"}
            self.assertFalse(self.check(root=root))

    def test_requirement_cycle_and_missing_edge_rejected(self):
        self.record()["depends_on"] = ["audit:FND-001:1", "nonexistent"]
        self.record("audit:FND-001:1")["depends_on"] = ["backlog:FND-001:1"]
        errors = self.check()
        self.assertTrue(any("cycle" in e for e in errors))
        self.assertTrue(any("missing dependency" in e for e in errors))

    def test_validated_cannot_hide_unfinished_dependency(self):
        self.validated()["depends_on"] = ["audit:FND-001:1"]
        self.assertTrue(any("unfinished dependency" in e for e in self.check()))

    def test_dag_cycles_and_phase_skipping_rejected(self):
        self.graph["nodes"][0]["depends_on"] = ["phase-8"]
        self.assertTrue(any("cycle" in e for e in self.check()))
        self.graph = copy.deepcopy(self.dag)
        next(n for n in self.graph["nodes"] if n["id"] == "phase-1")["depends_on"] = []
        self.assertTrue(any("phase delivery order" in e for e in self.check()))

    def test_dag_unknown_requirement_rejected(self):
        self.graph["nodes"][0]["requirements"] = ["bare-ambiguous-id"]
        self.assertTrue(any("unknown requirement" in e for e in self.check()))

    def test_same_source_extraction_is_deterministic(self):
        self.assertEqual(self.data, ledger.extract(self.source))
        self.assertEqual(ledger.render_trace(self.data), ledger.render_trace(ledger.extract(self.source)))

    def test_unmodified_block_ids_survive_preceding_addition(self):
        modified = ledger.extract("An introductory note.\n\n" + self.source)
        before = next(r for r in self.data["records"] if r["original_id"] == "FND-001")
        after = next(r for r in modified["records"] if r["original_id"] == "FND-001")
        self.assertEqual(before["id"], after["id"])
        self.assertEqual(before["source"]["sha256"], after["source"]["sha256"])

    def test_feature_heading_cannot_hide_unfinished_children(self):
        evidence = copy.deepcopy(self.validated())
        self.record()["status"] = "NOT_STARTED"
        feature = self.record("catalog:INS-001:1")
        for key in ("status", "owner", "acceptance_cases", "implementation", "validation", "reviews"):
            feature[key] = evidence[key]
        self.assertTrue(feature["children"])
        self.assertTrue(any("unfinished mandatory descendant" in e for e in self.check()))
        feature["status"] = "IMPLEMENTED"
        self.assertTrue(any("unfinished mandatory descendant" in e for e in self.check()))

    def test_coverage_records_have_no_completion_status(self):
        record = next(r for r in self.data["records"] if r["role"] == "structure")
        self.assertIsNone(record["status"])
        record["status"] = "VALIDATED"
        self.assertTrue(any("coverage-only" in e for e in self.check()))

    def test_review_rejection_and_unresolved_major_block_completion(self):
        record = self.validated()
        record["reviews"][0]["disposition"] = "CHANGES_REQUESTED"
        record["reviews"][0]["findings"] = [{"severity": "BLOCKER", "resolution": "OPEN"},
                                             {"severity": "MAJOR", "resolution": "OPEN"}]
        errors = self.check()
        self.assertTrue(any("must be APPROVED" in e for e in errors))
        self.assertTrue(any("unresolved BLOCKER" in e for e in errors))
        self.assertTrue(any("unresolved MAJOR" in e for e in errors))

    def test_stale_evidence_and_missing_revision_fail(self):
        record = self.validated()
        record["implementation"][0]["sha256"] = "0" * 64
        record["reviews"][0].pop("revision")
        self.assertTrue(any("stale evidence digest" in e for e in self.check()))
        self.assertTrue(any("missing revision" in e for e in self.check()))

    def test_plan_cannot_be_implementation_evidence(self):
        self.validated()["implementation"] = [self.evidence("plan.md")]
        self.assertTrue(any("not implementation evidence" in e for e in self.check()))

    def test_external_blocker_cannot_use_unlinked_infrastructure_claim(self):
        record = self.record()
        record["status"] = "BLOCKED_EXTERNAL"
        record["blocker"] = {"dependency": "hardware", "reason": "absent", "resume_procedure": "test",
                             "evidence": self.evidence(), "surrounding_infrastructure": "trust me"}
        self.assertTrue(any("surrounding implementation evidence" in e for e in self.check()))
        self.assertTrue(any("surrounding contract_validation evidence" in e for e in self.check()))

    def test_blocker_and_adr_explanations_render(self):
        record = self.record()
        record["blocker"] = {"reason": "CUDA device physically absent", "evidence": self.evidence()}
        record["adr"] = {"reason": "Preserve reviewed compatibility", "path": "example.md"}
        trace = ledger.render_trace(self.data)
        self.assertIn("CUDA device physically absent", trace)
        self.assertIn("Preserve reviewed compatibility", trace)

    def test_malformed_collection_shapes_return_errors(self):
        self.data["records"] = {}
        self.assertTrue(self.check())
        self.data = copy.deepcopy(self.baseline)
        self.record()["depends_on"] = "not-a-list"
        self.assertTrue(self.check())


if __name__ == "__main__":
    unittest.main()
