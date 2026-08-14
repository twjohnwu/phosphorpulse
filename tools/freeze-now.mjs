const frozen = Number(process.env.FREEZE_NOW_MS);

if (!Number.isFinite(frozen)) {
  throw new Error("FREEZE_NOW_MS must be a finite number");
}

globalThis.Date.now = () => frozen;
