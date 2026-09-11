"use strict";
// Bounded OFA colocation move: frontend/lib/ofa → frontend/features/ofa
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const fe = path.join(here, "..", "..", "frontend");
const src = path.join(fe, "lib", "ofa");
const dst = path.join(fe, "features", "ofa");
const isDst = path.join(dst, "index.ts");

const SKIP = /node_modules|\.next|\.git/;

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (SKIP.test(e.name)) continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (/\.(ts|tsx|js|jsx|mjs|cjs)$/.test(e.name)) out.push(p);
  }
  return out;
}

if (!fs.existsSync(src)) {
  console.log("✗ no src dir:", src);
  process.exit(1);
}

// 1) capture list
const ofaFiles = fs
  .readdirSync(src)
  .filter((f) => /\.(ts|tsx)$/.test(f))
  .sort();
console.log("── moving files:");
ofaFiles.forEach((f) => console.log("   • " + f));

// 2) create dest, move
fs.mkdirSync(dst, { recursive: true });
ofaFiles.forEach((f) => fs.renameSync(path.join(src, f), path.join(dst, f)));

// 3) rewrite import specifiers across the whole frontend
const oldPrefix = "@/lib/ofa";
const newPrefix = "@/features/ofa";
let touched = 0;
walk(fe).forEach((p) => {
  let s = fs.readFileSync(p, "utf8");
  if (!s.includes(oldPrefix)) return;
  const next = s.split(oldPrefix).join(newPrefix);
  fs.writeFileSync(p, next, "utf8");
  touched++;
  console.log("   ↳ rewrote " + path.relative(fe, p));
});
console.log(`── import rewrite done (${touched} file(s))`);

// 4) tidy: remove now-empty old dir if nothing remains
const remains = fs.readdirSync(src).filter((f) => f !== ".git"); // expect: none
if (remains.length === 0) {
  fs.rmdirSync(src);
  console.log("   ↳ removed empty " + path.relative(fe, src));
}

// 5) health: moved file exists at dst + old src gone
console.log("── health checks:");
console.log(
  "   dst index exists:  " + fs.existsSync(isDst),
);
ofaFiles.forEach((f) =>
  console.log(
    "   dst/"+f + " exists: " + fs.existsSync(path.join(dst, f)),
  ),
);
console.log("   old src exists:  " + fs.existsSync(src));
