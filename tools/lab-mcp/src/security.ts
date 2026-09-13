import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
  verify,
  type KeyObject,
} from "node:crypto";

export function canonical(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  const record = value as Record<string, unknown>;
  return `{${Object.keys(record)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${canonical(record[key])}`)
    .join(",")}}`;
}

export function digest(data: Uint8Array | string): string {
  return `sha256:${createHash("sha256").update(data).digest("hex")}`;
}

export function privateKeyFromEnv(name: string): KeyObject {
  const encoded = process.env[name];
  if (!encoded)
    throw new Error(`required signing credential is unavailable: ${name}`);
  return createPrivateKey({
    key: Buffer.from(encoded, "base64"),
    format: "der",
    type: "pkcs8",
  });
}

export function publicKeyFromEnv(name: string): KeyObject {
  const encoded = process.env[name];
  if (!encoded)
    throw new Error(`required verification credential is unavailable: ${name}`);
  return createPublicKey({
    key: Buffer.from(encoded, "base64"),
    format: "der",
    type: "spki",
  });
}

export function signObject(payload: object, key: KeyObject): string {
  return sign(null, Buffer.from(canonical(payload)), key).toString("base64");
}

export function verifyObject(
  payload: object,
  signature: string,
  key: KeyObject,
): boolean {
  try {
    return verify(
      null,
      Buffer.from(canonical(payload)),
      key,
      Buffer.from(signature, "base64"),
    );
  } catch {
    return false;
  }
}

export function authenticateHostResponse(
  payload: object,
  signature: string,
  publicKey: KeyObject,
  expectedIdentity: string,
): void {
  const actual = `sha256:${createHash("sha256")
    .update(publicKey.export({ format: "der", type: "spki" }))
    .digest("base64")}`;
  if (actual !== expectedIdentity)
    throw new Error("host identity does not match its pinned identity");
  if (!verifyObject(payload, signature, publicKey))
    throw new Error("host response signature invalid");
}

export function sanitize(
  text: string,
  maxBytes: number,
): { text: string; truncated: boolean; policy: string } {
  let value = text
    .replace(/\b(?:[0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}\b/g, "[redacted-mac]")
    .replace(/\b(?:\d{1,3}\.){3}\d{1,3}\b/g, "[redacted-ip]")
    .replace(/\bSSID\s*[:=]\s*[^\r\n]+/gi, "SSID=[redacted]")
    .replace(
      /(token|password|secret|authorization)\s*[:=]\s*\S+/gi,
      "$1=[redacted]",
    );
  const bytes = Buffer.from(value);
  const truncated = bytes.length > maxBytes;
  if (truncated)
    value = bytes
      .subarray(0, maxBytes)
      .toString("utf8")
      .replace(/\uFFFD$/, "");
  return { text: value, truncated, policy: "kyberia-lab-text-v1" };
}
