#!/usr/bin/env node
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { access, mkdir } from "node:fs/promises";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const reference = "/Users/sinbaji/Desktop/FullStack/sideProjects/phosphorflux";
const freezeNow = join(root, "tools", "freeze-now.mjs");

function fail(message) {
  process.stderr.write(`capture-golden: ${message}\n`);
  process.exitCode = 1;
}

function run(command, args, options) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(command, args, options);
    const stdout = [];
    const stderr = [];
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.on("error", reject);
    child.on("close", (code, signal) => resolveRun({ code, signal, stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr) }));
  });
}

async function capture(sampleDir) {
  const [meta, envSpec, stdin] = await Promise.all([
    readFile(join(sampleDir, "meta.json"), "utf8").then(JSON.parse),
    readFile(join(sampleDir, "env.json"), "utf8").then(JSON.parse),
    readFile(join(sampleDir, "stdin.json")),
  ]);
  if (meta.mode !== "render" && meta.mode !== "subagent") throw new Error(`invalid meta mode in ${sampleDir}`);
  for (const key of ["COLUMNS", "TERM", "FREEZE_NOW_MS"]) {
    if (typeof envSpec[key] !== "string") throw new Error(`${join(sampleDir, "env.json")} lacks string ${key}`);
  }
  const home = await mkdtemp(join(tmpdir(), "phosphorpulse-golden-"));
  const configDir = join(home, "config");
  try {
    await mkdir(configDir, { recursive: true });
    await cp(join(sampleDir, "settings.json"), join(configDir, "settings.json"));
    try { await access(join(sampleDir, "state"), fsConstants.F_OK); await cp(join(sampleDir, "state"), configDir, { recursive: true }); } catch (error) { if (error?.code !== "ENOENT") throw error; }
    const env = {
      HOME: home,
      PPF_CONFIG_DIR: configDir,
      PATH: join(sampleDir, "bin"),
    };
    for (const [key, value] of Object.entries(envSpec)) if (key !== "now_ms" && !(key === "COLORTERM" && value === "") && typeof value === "string") env[key] = value;
    const args = ["--import", freezeNow, join(reference, "dist", "cli.js"), "render"];
    if (meta.mode === "subagent") args.push("--subagent");
    const result = await run(process.execPath, args, { cwd: join(sampleDir, "cwd"), env, input: undefined });
    // spawn does not accept stdin bytes in its options; feed it using a small second process path below.
    return { result, stdin, env, args, cwd: join(sampleDir, "cwd") };
  } finally { await rm(home, { recursive: true, force: true }); }
}

function runWithInput(command, args, { cwd, env, input }) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(command, args, { cwd, env, stdio: ["pipe", "pipe", "pipe"] });
    const stdout = []; const stderr = [];
    child.stdout.on("data", (chunk) => stdout.push(chunk)); child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.on("error", reject); child.on("close", (code, signal) => resolveRun({ code, signal, stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr) }));
    child.stdin.end(input);
  });
}

async function oneCapture(sampleDir) {
  const [meta, envSpec, stdin] = await Promise.all([readFile(join(sampleDir, "meta.json"), "utf8").then(JSON.parse), readFile(join(sampleDir, "env.json"), "utf8").then(JSON.parse), readFile(join(sampleDir, "stdin.json"))]);
  const home = await mkdtemp(join(tmpdir(), "phosphorpulse-golden-"));
  const configDir = join(home, "config");
  try {
    await mkdir(configDir, { recursive: true });
    await cp(join(sampleDir, "settings.json"), join(configDir, "settings.json"));
    try { await access(join(sampleDir, "state")); await cp(join(sampleDir, "state"), configDir, { recursive: true }); } catch (error) { if (error?.code !== "ENOENT") throw error; }
    const env = { HOME: home, PPF_CONFIG_DIR: configDir, PATH: join(sampleDir, "bin") };
    for (const [key, value] of Object.entries(envSpec)) if (key !== "now_ms" && !(key === "COLORTERM" && value === "") && typeof value === "string") env[key] = value;
    const args = ["--import", freezeNow, join(reference, "dist", "cli.js"), "render", ...(meta.mode === "subagent" ? ["--subagent"] : [])];
    const result = await runWithInput(process.execPath, args, { cwd: join(sampleDir, "cwd"), env, input: stdin });
    if (result.code !== 0) throw new Error(`render exited ${result.code ?? result.signal}: ${result.stderr.toString("utf8")}`);
    return result.stdout;
  } finally { await rm(home, { recursive: true, force: true }); }
}

async function cmpBytes(first, second) {
  const dir = await mkdtemp(join(tmpdir(), "phosphorpulse-cmp-"));
  try {
    const left = join(dir, "first.bin"); const right = join(dir, "second.bin");
    await Promise.all([writeFile(left, first), writeFile(right, second)]);
    const compared = await run("cmp", ["--", left, right], { stdio: ["ignore", "pipe", "pipe"] });
    return compared.code === 0;
  } finally { await rm(dir, { recursive: true, force: true }); }
}

const sampleDir = process.argv[2] ? resolve(process.argv[2]) : undefined;
if (!sampleDir) { fail("usage: node tools/capture-golden.mjs tests/golden/<sample>"); }
else {
  try {
    const first = await oneCapture(sampleDir);
    const second = await oneCapture(sampleDir);
    if (!(await cmpBytes(first, second))) {
      let offset = 0; while (offset < first.length && offset < second.length && first[offset] === second[offset]) offset++;
      throw new Error(`non-reproducible sample: two captures differ for ${sampleDir} at byte ${offset} (${first.subarray(Math.max(0, offset - 24), offset + 48).toString("hex")} != ${second.subarray(Math.max(0, offset - 24), offset + 48).toString("hex")})`);
    }
    const { readFile: readMeta } = await import("node:fs/promises");
    const sampleMeta = JSON.parse(await readMeta(join(sampleDir, "meta.json"), "utf8"));
    if (sampleMeta.mode === "subagent") {
      // Frozen TS `render --subagent` emits JSON with ANSI escaped as  — raw 0x1b never appears (reference behavior).
      const text = first.toString("utf8");
      if (first.length === 0) throw new Error(`invalid golden output: empty subagent output for ${sampleDir}`);
      JSON.parse(text); // must be valid JSON
      if (!text.includes("\\u001b")) throw new Error(`invalid golden output: subagent JSON lacks escaped ANSI for ${sampleDir}`);
    } else if (first.length === 0 || !first.includes(0x1b)) throw new Error(`invalid golden output: expected non-empty ANSI bytes for ${sampleDir}: ${first.subarray(0, 160).toString("utf8")}`);
    await writeFile(join(sampleDir, "expected.bin"), first);
    process.stdout.write(`${sampleDir}: captured ${first.length} bytes; cmp: identical\n`);
  } catch (error) { fail(error instanceof Error ? error.message : String(error)); }
}
