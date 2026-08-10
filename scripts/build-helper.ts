/**
 * Builds `gp-helper` and stages it where Tauri expects a sidecar.
 *
 * Tauri only ships binaries listed in `bundle.externalBin`, and it looks each
 * one up by target triple. Without this staging step the helper is built into
 * `target/` but never copied into the .deb, so the installed app spawns
 * `/usr/bin/gp-helper` and finds nothing there.
 */
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = join(root, "src-tauri", "Cargo.toml");

const release = process.argv.includes("--release");
const targetFlag = process.argv.indexOf("--target");
const target = targetFlag === -1 ? hostTriple() : process.argv[targetFlag + 1];
const suffix = target.includes("windows") ? ".exe" : "";

function hostTriple(): string {
	const version = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
	const host = version.match(/^host:\s*(\S+)$/m);
	if (!host) throw new Error("could not read the host triple from `rustc -vV`");
	return host[1];
}

const args = ["build", "--manifest-path", manifest, "-p", "gp-helper"];
if (release) args.push("--release");
if (targetFlag !== -1) args.push("--target", target);
execFileSync("cargo", args, { stdio: "inherit" });

// `--target` gives cargo a per-triple output directory; a host build has none.
const profile = release ? "release" : "debug";
const built = join(
	root,
	"src-tauri",
	"target",
	...(targetFlag === -1 ? [profile] : [target, profile]),
	`gp-helper${suffix}`,
);
const staged = join(root, "src-tauri", "binaries", `gp-helper-${target}${suffix}`);

mkdirSync(dirname(staged), { recursive: true });
copyFileSync(built, staged);
console.log(`staged ${built} -> ${staged}`);
