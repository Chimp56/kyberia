import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
  verify,
  type KeyObject,
} from "node:crypto";
import { lstat, readFile } from "node:fs/promises";

export function canonical(value: unknown): string {
  if (value === null) return "null";
  if (typeof value === "number") {
    if (!Number.isFinite(value))
      throw new Error("non-finite number is not canonicalizable");
    return JSON.stringify(value);
  }
  if (typeof value === "string" || typeof value === "boolean")
    return JSON.stringify(value);
  if (typeof value !== "object" || value === undefined)
    throw new Error("unsupported value is not canonicalizable");
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (Object.getPrototypeOf(value) !== Object.prototype)
    throw new Error("non-plain object is not canonicalizable");
  const record = value as Record<string, unknown>;
  return `{${Object.keys(record)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${canonical(record[key])}`)
    .join(",")}}`;
}

export function digest(data: Uint8Array | string): string {
  return `sha256:${createHash("sha256").update(data).digest("hex")}`;
}

export async function verifyExecutable(
  path: string,
  expectedSha256: string,
): Promise<void> {
  const stat = await lstat(path);
  if (!stat.isFile() || stat.isSymbolicLink())
    throw new Error("trusted executable must be a regular non-symlink file");
  const actual = createHash("sha256")
    .update(await readFile(path))
    .digest("hex");
  if (actual !== expectedSha256)
    throw new Error("trusted executable digest mismatch");
}

export async function verifyInvocationArguments(
  arguments_: string[],
  pinnedFiles: Array<{ argumentIndex: number; sha256: string }>,
): Promise<void> {
  const pinned = new Map(
    pinnedFiles.map((item) => [item.argumentIndex, item.sha256]),
  );
  if (pinned.size !== pinnedFiles.length)
    throw new Error("argument file indices must be unique");
  for (const [index, argument] of arguments_.entries()) {
    if (["-e", "--eval", "-c", "--command"].includes(argument))
      throw new Error("inline code-bearing runner arguments are forbidden");
    const fileLike =
      /^(?:\/|[A-Za-z]:\\)/.test(argument) ||
      /\.(?:[cm]?js|tsx?|py|sh|bash|zsh|ps1)$/i.test(argument);
    if (fileLike && !pinned.has(index))
      throw new Error("code-bearing runner argument is not digest pinned");
  }
  for (const [index, sha256] of pinned) {
    const path = arguments_[index];
    if (!path || !/^(?:\/|[A-Za-z]:\\)/.test(path))
      throw new Error("pinned runner argument must be an absolute path");
    await verifyExecutable(path, sha256);
  }
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

export function publicKeyIdentity(key: KeyObject): string {
  return `sha256:${createHash("sha256")
    .update(key.export({ format: "der", type: "spki" }))
    .digest("base64")}`;
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
  const actual = publicKeyIdentity(publicKey);
  if (actual !== expectedIdentity)
    throw new Error("host identity does not match its pinned identity");
  if (!verifyObject(payload, signature, publicKey))
    throw new Error("host response signature invalid");
}

export function sanitize(
  text: string,
  maxBytes: number,
): { text: string; truncated: boolean; policy: string } {
  if (/\0|\uFFFD/.test(text))
    throw new Error("non-text artifact rejected by deny-by-default policy");
  // Bound the working set before applying regular expressions. The extra tail
  // lets a token crossing the retained boundary be fully redacted.
  const source = Buffer.from(text)
    .subarray(0, maxBytes + 4096)
    .toString("utf8");
  let value = source
    .replace(/\b(?:[0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}\b/g, "[redacted-mac]")
    .replace(/\b(?:[0-9A-Fa-f]{2}-){5}[0-9A-Fa-f]{2}\b/g, "[redacted-mac]")
    .replace(/\b(?:\d{1,3}\.){3}\d{1,3}\b/g, "[redacted-ip]")
    .replace(
      /(?<![A-Za-z0-9])(?:[A-Fa-f0-9]{0,4}:){2,7}[A-Fa-f0-9]{0,4}(?![A-Za-z0-9])/g,
      "[redacted-ip]",
    )
    .replace(/\bSSID\s*[:=]\s*[^\r\n]+/gi, "SSID=[redacted]")
    .replace(/\bAKIA[0-9A-Z]{16}\b/g, "[redacted-cloud-credential]")
    .replace(
      /\b(?:Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+/gi,
      "[redacted-authorization]",
    )
    .replace(
      /(["'](?:ssid|psk|token|access[_-]?token|refresh[_-]?token|password|secret|aws[_-]?secret[_-]?access[_-]?key|authorization|api[_-]?key)["']\s*:\s*)["'][^"'\r\n]*["']/gi,
      '$1"[redacted]"',
    )
    .replace(
      /(token|password|secret|authorization)\s*[:=]\s*\S+/gi,
      "$1=[redacted]",
    )
    .replace(
      /\b(?:gh[pousr]_[A-Za-z0-9]{20,}|xox[baprs]-[A-Za-z0-9-]{20,})\b/g,
      "[redacted-token]",
    )
    .replace(
      /(?:[A-Za-z]:\\|\/Users\/|\/home\/|\/private\/|\/var\/|\/tmp\/)[^\s"']+/g,
      "[redacted-path]",
    );
  const bytes = Buffer.from(value);
  const truncated = bytes.length > maxBytes;
  if (truncated)
    value = bytes
      .subarray(0, maxBytes)
      .toString("utf8")
      .replace(/\uFFFD$/, "");
  return {
    text: value,
    truncated: truncated || Buffer.byteLength(text) > maxBytes,
    policy: "kyberia-lab-text-v2",
  };
}
