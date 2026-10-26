import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const env = { ...process.env };
const originalPath =
  Object.entries(env).find(([key]) => key.toLowerCase() === "path")?.[1] || "";
for (const key of Object.keys(env)) {
  if (key.toLowerCase() === "path") delete env[key];
}
env.Path = path.dirname(process.execPath) + path.delimiter + originalPath;
const cargo = path.join(root, ".tools", "cargo");
if (existsSync(cargo)) {
  env.CARGO_HOME = cargo;
  env.RUSTUP_HOME = path.join(root, ".tools", "rustup");
  env.Path = path.join(cargo, "bin") + path.delimiter + env.Path;
}
env.CARGO_BUILD_JOBS = "4";
const result = spawnSync(
  process.execPath,
  [
    path.join(root, "node_modules", "@tauri-apps", "cli", "tauri.js"),
    ...process.argv.slice(2),
  ],
  {
    cwd: path.join(root, "apps", "desktop"),
    env,
    stdio: "inherit",
    windowsHide: true,
  },
);
if (result.error) console.error(result.error.message);
process.exit(result.status ?? 1);
