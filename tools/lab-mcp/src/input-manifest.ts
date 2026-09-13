import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { canonical, digest, verifyExecutable } from "./security.js";

const MAX_TREE_BYTES = 16_777_216;
const MAX_FILE_BYTES = 1_073_741_824;
const MAX_FILES = 20_000;

function git(
  executable: string,
  repository: string,
  args: string[],
  maxBuffer: number,
): Promise<Buffer> {
  return new Promise((resolve, reject) =>
    execFile(
      executable,
      args,
      { cwd: repository, encoding: "buffer", maxBuffer, env: {} },
      (error, stdout) => (error ? reject(error) : resolve(stdout as Buffer)),
    ),
  );
}

export async function generateInputManifest(
  executable: string,
  executableSha256: string,
  repository: string,
  revision: string,
) {
  if (!/^[0-9a-f]{40}$/.test(revision)) throw new Error("exact SHA required");
  await verifyExecutable(executable, executableSha256);
  const objectType = (
    await git(executable, repository, ["cat-file", "-t", revision], 4_096)
  )
    .toString("utf8")
    .trim();
  if (objectType !== "commit") throw new Error("revision is not a commit");
  const listing = await git(
    executable,
    repository,
    ["ls-tree", "-rz", "--full-tree", revision],
    MAX_TREE_BYTES,
  );
  const records = listing.toString("utf8").split("\0").filter(Boolean);
  if (!records.length || records.length > MAX_FILES)
    throw new Error("commit tree file count rejected");
  let total = 0;
  const entries = [];
  for (const record of records) {
    const match = /^(100644|100755) blob ([0-9a-f]{40,64})\t(.+)$/.exec(record);
    if (!match) throw new Error("symlink, submodule, or unsafe entry rejected");
    const bytes = await git(
      executable,
      repository,
      ["cat-file", "blob", match[2]!],
      MAX_FILE_BYTES,
    );
    total += bytes.length;
    if (total > MAX_FILE_BYTES)
      throw new Error("commit tree byte limit exceeded");
    entries.push({
      path: match[3]!,
      mode: match[1] as "100644" | "100755",
      sha256: createHash("sha256").update(bytes).digest("hex"),
    });
  }
  entries.sort((a, b) => a.path.localeCompare(b.path));
  return { inputManifestId: digest(canonical(entries)), entries };
}

if (import.meta.url === new URL(process.argv[1] ?? "", "file:").href) {
  const [executable, executableSha256, repository, revision, ...extra] =
    process.argv.slice(2);
  if (
    !executable ||
    !executableSha256 ||
    !repository ||
    !revision ||
    extra.length
  )
    throw new Error(
      "usage: input-manifest <absolute-git> <git-sha256> <repository> <40-hex-revision>",
    );
  process.stdout.write(
    `${canonical(await generateInputManifest(executable, executableSha256, repository, revision))}\n`,
  );
}
