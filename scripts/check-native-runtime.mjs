import { readFile } from "node:fs/promises";
import path from "node:path";
import { externalVcRuntimes, peImports } from "./lib/pe-imports.mjs";

const projectRoot = path.resolve(import.meta.dirname, "..");
const files = process.argv.slice(2);
if (!files.length) {
  files.push(
    path.join(projectRoot, "src-tauri/target/release/creel-desktop-host.exe"),
    path.join(projectRoot, "src-tauri/target/release/creel_shell.dll")
  );
}

for (const file of files) {
  try {
    const imports = peImports(await readFile(file));
    const external = externalVcRuntimes(imports);
    if (external.length) {
      throw new Error(`仍依赖外部 VC++ 运行库：${external.join(", ")}`);
    }
    console.log(`${path.basename(file)}：通过 VC++ 运行库依赖检查`);
  } catch (error) {
    console.error(`${file}：${error.message}`);
    process.exitCode = 1;
  }
}
