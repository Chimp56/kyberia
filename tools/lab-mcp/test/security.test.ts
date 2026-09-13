import assert from "node:assert/strict";
import { test } from "node:test";
import { canonical, sanitize } from "../src/security.js";

test("canonical form rejects undefined, non-finite numbers, and exotic objects", () => {
  assert.throws(() => canonical({ value: undefined }), /unsupported/);
  assert.throws(() => canonical({ value: Number.NaN }), /non-finite/);
  assert.throws(
    () => canonical({ value: Number.POSITIVE_INFINITY }),
    /non-finite/,
  );
  assert.throws(() => canonical(new Date()), /plain/);
  assert.equal(canonical({ b: 2, a: [true, null] }), '{"a":[true,null],"b":2}');
});

test("sanitizer covers structured credentials, addresses, and local paths", () => {
  const raw = [
    '{"password":"hunter2","api_key":"cloud-secret"}',
    "Bearer eyJhbGciOi.secret",
    "Basic dXNlcjpwYXNz",
    "AKIAIOSFODNN7EXAMPLE",
    "aa-bb-cc-dd-ee-ff",
    "2001:db8:85a3::8a2e:370:7334",
    "/Users/alice/private/project.txt",
    "C:\\Users\\alice\\secret.txt",
  ].join("\n");
  const result = sanitize(raw, 4096);
  assert.doesNotMatch(
    result.text,
    /hunter2|cloud-secret|eyJhbGci|dXNlc|AKIAIOS|aa-bb|2001:db8|alice/,
  );
  assert.match(result.text, /redacted/);
  assert.equal(result.policy, "kyberia-lab-text-v2");
});

test("sanitizer bounds its output and reports truncation", () => {
  const result = sanitize("x".repeat(1_000_000), 1024);
  assert.ok(Buffer.byteLength(result.text) <= 1024);
  assert.equal(result.truncated, true);
});
