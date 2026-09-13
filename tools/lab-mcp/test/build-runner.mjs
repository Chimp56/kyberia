import { build } from "esbuild";

await build({
  entryPoints: [new URL("../src/runner.ts", import.meta.url).pathname],
  outfile: new URL("../dist/runner-bundle.mjs", import.meta.url).pathname,
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node24",
  sourcemap: false,
  legalComments: "none",
});
