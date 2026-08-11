import { copyFile, mkdir } from "node:fs/promises";
import { build } from "esbuild";

await mkdir("dist/static", { recursive: true });

await build({
  entryPoints: ["browser/app.ts"],
  outfile: "dist/static/app.js",
  bundle: true,
  minify: true,
  format: "esm",
  target: "es2022",
  platform: "browser",
  // The web UI only calls lint(), never convert()/inspectTemplate() — but @compyla/sdk's dynamic
  // `import("@compyla/docx")` etc. (opt-in renderer/introspector loading) are static-string dynamic
  // imports, which esbuild inlines into the bundle by default. Since there's no splitting target for
  // them to go to, that drags in each renderer's Node-only dependencies (image-size, pdfkit's zlib
  // shims, …) and the build fails resolving Node builtins for the browser platform. Those imports are
  // never reached at runtime here, so excluding them from the bundle is correct, not just expedient.
  external: ["@compyla/docx", "@compyla/pdf", "@compyla/pptx", "@compyla/xlsx"],
});

await copyFile("browser/index.html", "dist/static/index.html");
await copyFile("browser/style.css", "dist/static/style.css");
