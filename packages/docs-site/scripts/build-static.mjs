import { writeFile, mkdir, copyFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { build } from "esbuild";
import { generateSite } from "../dist/generate.js";

const outDir = "dist/static";

const { files } = await generateSite();
for (const [relPath, content] of files) {
  const outPath = join(outDir, relPath);
  await mkdir(dirname(outPath), { recursive: true });
  await writeFile(outPath, content);
}

await build({
  entryPoints: ["browser/app.ts"],
  outfile: join(outDir, "assets/app.js"),
  bundle: true,
  minify: true,
  format: "esm",
  target: "es2022",
  platform: "browser",
});

await copyFile("browser/style.css", join(outDir, "assets/style.css"));
