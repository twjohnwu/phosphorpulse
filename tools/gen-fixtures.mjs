#!/usr/bin/env node
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const reference = "/Users/sinbaji/Desktop/FullStack/sideProjects/phosphorflux";
const outDir = join(root, "tests", "fixtures");
const widthInputs = ["", "A", "hello", "中", "中文", "台灣", "🇹🇼", "❤️", "♥", "👨‍👩‍👧‍👦", "👩‍💻", "😀", "a中b", "ＡＢＣ", "AＡ中", "é", "न", "한글", "￥", "ｶ", "\u001b[31mred\u001b[0m", "x\u001b[1;34my\u001b[0mz", "\n", "\t", "©", "™️", "🏳️‍🌈", "👨‍👩‍👧‍👦中", "🇹🇼❤️", "abc中def", "𠀀", "·"];
const paletteInputs = ["#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff", "#808080", "#c0c0c0", "#121612", "#0e120e", "#0a0e0a", "#00cf41", "#00cdcd", "#00ffff", "#969696", "#008f11", "#00e5ff", "#ffd700", "#ff7f50", "#ff3737", "#268bd2", "#2aa198", "#b58900", "#93a1a1", "#586e75", "#859900", "#073642", "#657b83", "#cb4b16", "#6c71c4", "#839496", "#eee8d5", "#dc322f", "#123456", "#abcdef", "#fedcba", "#102030", "#fefefe", "#010203", "#7f3f00", "#3f7fff", "#cc44aa"];
const numberInputs = [0.125, 2.675, 1.005, -0.125, -2.675, -1.005, 0, -0, 1, -1, 1.335, 10.235, 999.995, 1e21, "0x10", " 12 ", "", "1e21", "Infinity", "-Infinity", "NaN", null, true, false, "0b101", "0o10", "-0.125", "2.675", "1.005", "  ", "foo"];

function runTsx(command, args, script) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(command, args, { cwd: reference, env: { ...process.env, TMPDIR: "/private/tmp" }, stdio: ["ignore", "pipe", "pipe"] });
    const out = []; const err = [];
    child.stdout.on("data", c => out.push(c)); child.stderr.on("data", c => err.push(c)); child.on("error", reject);
    child.on("close", code => code === 0 ? resolveRun(Buffer.concat(out).toString("utf8")) : reject(new Error(Buffer.concat(err).toString("utf8"))));
  });
}

async function tsx(script) {
  // Normal execution deliberately uses the project's `npx tsx`; the loader
  // fallback is for sandboxes that forbid tsx's Unix-socket IPC daemon.
  try { return await runTsx("npx", ["--no-install", "tsx", "-e", script], script); }
  catch (error) {
    if (!String(error).includes("EPERM")) throw error;
    return runTsx(process.execPath, ["--import", "tsx", "-e", script], script);
  }
}

const widthScript = `import { displayWidth } from './src/render/width.ts'; const xs=${JSON.stringify(widthInputs)}; console.log(JSON.stringify(xs.map(input=>({input,width:displayWidth(input)}))))`;
const paletteScript = `import { encodeColor } from './src/color/ansi.ts'; const xs=${JSON.stringify(paletteInputs)}; console.log(JSON.stringify(xs.map(input=>({input,color256:encodeColor(input,'256color','fg'),color16:encodeColor(input,'16color','fg')}))))`;
function printable(value) { return typeof value === "number" ? (Number.isNaN(value) ? "NaN" : value === Infinity ? "Infinity" : value === -Infinity ? "-Infinity" : Object.is(value, -0) ? "-0" : String(value)) : String(value); }
function numberRow(input) { const n = Number(input); return { input: printable(input), toFixed2: Number.isFinite(n) ? n.toFixed(2) : String(n), toNumber: printable(n) }; }

try {
  const [width, palette] = await Promise.all([tsx(widthScript), tsx(paletteScript)]);
  await writeFile(join(outDir, "width-table.json"), `${JSON.stringify(JSON.parse(width), null, 2)}\n`);
  await writeFile(join(outDir, "palette-table.json"), `${JSON.stringify(JSON.parse(palette), null, 2)}\n`);
  await writeFile(join(outDir, "number-table.json"), `${JSON.stringify(numberInputs.map(numberRow), null, 2)}\n`);
  process.stdout.write(`fixtures: width=${widthInputs.length} number=${numberInputs.length} palette=${paletteInputs.length}\n`);
} catch (error) { process.stderr.write(`gen-fixtures: ${error instanceof Error ? error.message : String(error)}\n`); process.exitCode = 1; }
