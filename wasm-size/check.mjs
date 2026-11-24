// Runs the size-check module's three entry points over the catalogue check string and a longer
// input, against the catalogue check value and against each other.
import { readFileSync } from "node:fs";

const path = process.argv[2];
const { instance } = await WebAssembly.instantiate(readFileSync(path));
const { memory, one_shot, helper, streaming } = instance.exports;

function run(fn, bytes) {
  const view = new Uint8Array(memory.buffer, 1 << 16, bytes.length);
  view.set(bytes);
  return BigInt.asUintN(64, fn(1 << 16, bytes.length));
}

const check = new TextEncoder().encode("123456789");
for (const [name, fn] of [["one_shot", one_shot], ["helper", helper], ["streaming", streaming]]) {
  const got = run(fn, check);
  if (got !== 0xae8b14860a799888n) {
    throw new Error(`${name}: check value ${got.toString(16)}`);
  }
}
const long = Uint8Array.from({ length: 4099 }, (_, i) => (i * 131 + 7) & 0xff);
const expected = run(one_shot, long);
for (const [name, fn] of [["helper", helper], ["streaming", streaming]]) {
  if (run(fn, long) !== expected) {
    throw new Error(`${name} disagrees with one_shot on 4099 bytes`);
  }
}
console.log("CRC-64/NVME check values match");
