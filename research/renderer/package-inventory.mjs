#!/usr/bin/env node
/**
 * Verify or refresh the resolved package metadata used by this research harness.
 *
 * The lockfile is the authority for the package/version/integrity set.  pnpm's
 * The default command is an offline verification of the retained inventory
 * against the lockfile. `--refresh` is the explicit network-enabled command
 * that rebuilds metadata for a changed lockfile.
 */
import { createHash } from 'node:crypto';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

const lockText = await readFile('pnpm-lock.yaml', 'utf8');
const packageSection = lockText.split('\npackages:\n')[1]?.split('\nsnapshots:\n')[0];
if (!packageSection) throw new Error('pnpm-lock.yaml has no packages section');

const lines = packageSection.split('\n');
const records = [];
for (let index = 0; index < lines.length; index += 1) {
  const match = /^  (\S.+):$/.exec(lines[index]);
  if (!match) continue;
  const key = match[1].replace(/^['"]|['"]$/g, '');
  const body = [];
  for (let cursor = index + 1; cursor < lines.length && !/^  \S.+:$/.test(lines[cursor]); cursor += 1) body.push(lines[cursor]);
  const at = key.lastIndexOf('@');
  if (at <= 0) throw new Error(`Cannot parse lock package key: ${key}`);
  const name = key.slice(0, at);
  const version = key.slice(at + 1).split('_', 1)[0];
  const integrity = body.join('\n').match(/integrity:\s+([^\s}]+)/)?.[1] || null;
  const conditions = {};
  for (const field of ['os', 'cpu', 'libc']) {
    const value = body.join('\n').match(new RegExp(`^    ${field}: \\[(.+)\\]$`, 'm'))?.[1];
    if (value) conditions[field] = value.split(',').map((item) => item.trim());
  }
  records.push({ key, name, version, integrity, conditions });
}

const requiredFields = ['name', 'version', 'lockKey', 'license', 'source', 'archive', 'integrity', 'packageJsonSha256', 'redistribution', 'update'];
if (!process.argv.includes('--refresh')) {
  const retained = JSON.parse(await readFile('package-inventory.generated.json', 'utf8'));
  if (retained.packageCount !== records.length || retained.packages?.length !== records.length) throw new Error(`offline inventory count mismatch: lock=${records.length} inventory=${retained.packages?.length}`);
  const byKey = new Map(retained.packages.map((item) => [item.lockKey, item]));
  for (const record of records) {
    const item = byKey.get(record.key);
    if (!item) throw new Error(`offline inventory missing ${record.key}`);
    if (item.name !== record.name || item.version !== record.version || item.integrity !== record.integrity) throw new Error(`offline inventory lock mismatch for ${record.key}`);
    for (const field of requiredFields) if (!item[field] || String(item[field]).includes('UNKNOWN')) throw new Error(`offline inventory missing ${field} for ${record.key}`);
  }
  if (byKey.size !== records.length) throw new Error('offline inventory contains a package absent from the lockfile');
  console.log(JSON.stringify({ mode: 'offline-verify', packageCount: records.length, registryAccess: false, inventory: 'package-inventory.generated.json' }));
  process.exit(0);
}

const storeDirs = await readdir('node_modules/.pnpm');
const packageMetadata = new Map();
for (const record of records) {
  const storeKey = `${record.name.replaceAll('/', '+')}@${record.version}`;
  const candidates = storeDirs.filter((directory) => directory === storeKey || directory.startsWith(`${storeKey}_`));
  let metadata = null;
  let metadataPath = null;
  for (const directory of candidates) {
    const path = join('node_modules', '.pnpm', directory, 'node_modules', record.name, 'package.json');
    try {
      metadata = JSON.parse(await readFile(path, 'utf8'));
      metadataPath = path;
      break;
    } catch {
      // A platform package can be in the lockfile but omitted from this host's
      // linked tree; its package metadata is still expected in the pnpm store.
    }
  }
  if (!metadata) {
    const registryName = record.name.replaceAll('/', '%2f');
    const response = await fetch(`https://registry.npmjs.org/${registryName}`);
    if (!response.ok) throw new Error(`Missing local package metadata for ${record.key}; registry response ${response.status}`);
    const packageDocument = await response.json();
    metadata = packageDocument.versions?.[record.version];
    metadataPath = `https://registry.npmjs.org/${registryName}#${record.version}`;
    if (!metadata) throw new Error(`Registry has no metadata for ${record.key}`);
  }
  packageMetadata.set(record.key, { metadata, metadataPath });
}

const normalizeRepository = (repository) => {
  if (!repository) return null;
  if (typeof repository === 'string') return repository;
  return repository.url || null;
};
const archiveUrl = (name, version) => `https://registry.npmjs.org/${name}/-/${name.split('/').pop()}-${version}.tgz`;
const sourceUrl = (name, version, metadata) => normalizeRepository(metadata.repository) || metadata.homepage || `https://www.npmjs.com/package/${name}/v/${version}`;

const packages = records.map((record) => {
  const { metadata, metadataPath } = packageMetadata.get(record.key);
  const metadataBytes = Buffer.from(JSON.stringify(metadata));
  return {
    name: record.name,
    version: record.version,
    lockKey: record.key,
    license: metadata.license || metadata.licenses || 'UNKNOWN (package metadata did not declare a license)',
    source: sourceUrl(record.name, record.version, metadata),
    repository: normalizeRepository(metadata.repository),
    archive: archiveUrl(record.name, record.version),
    integrity: record.integrity,
    metadataSource: metadataPath,
    packageJsonSha256: createHash('sha256').update(metadataBytes).digest('hex'),
    conditions: record.conditions,
    redistribution: 'Dependency remains an external pinned npm package; this harness does not copy its source or assets.',
    update: 'On version or lockfile updates, regenerate this inventory, review license/source changes, and rerun the renderer proof.',
  };
});

const output = {
  generated: new Date().toISOString().slice(0, 10),
  lockfile: 'research/renderer/pnpm-lock.yaml',
  packageCount: packages.length,
  metadataPolicy: 'Every record in the lockfile packages section is listed, including platform-conditional packages. integrity is copied from pnpm-lock.yaml; archive is the npm registry tarball URL; source and license come from the resolved package.json in the pnpm store.',
  packages,
};
await writeFile('package-inventory.generated.json', JSON.stringify(output, null, 2));
console.log(JSON.stringify({ packageCount: output.packageCount, output: 'package-inventory.generated.json' }));
