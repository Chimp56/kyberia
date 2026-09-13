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
    '{"ssid":"private-wifi","psk":"wifi-password","password":"hunter2","api_key":"cloud-secret","aws_secret_access_key":"aws-secret","access_token":"ghp_abcdefghijklmnopqrstuvwxyz123456"}',
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
    /private-wifi|wifi-password|hunter2|cloud-secret|aws-secret|ghp_|eyJhbGci|dXNlc|AKIAIOS|aa-bb|2001:db8|alice/,
  );
  assert.match(result.text, /redacted/);
  assert.equal(result.policy, "kyberia-lab-text-v2");
});

test("sanitizer rejects binary-like output", () => {
  assert.throws(() => sanitize("pcap\0payload", 1024), /non-text/);
  assert.throws(() => sanitize("bad\uFFFDdecode", 1024), /non-text/);
});

test("sanitizer bounds its output and reports truncation", () => {
  const result = sanitize("x".repeat(1_000_000), 1024);
  assert.ok(Buffer.byteLength(result.text) <= 1024);
  assert.equal(result.truncated, true);
});
