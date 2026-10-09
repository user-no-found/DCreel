const assert = require("node:assert/strict");
const { test } = require("node:test");

function peFile({ delay = false, runtime = "VCRUNTIME140.dll", magic = 0x20b } = {}) {
  const bytes = Buffer.alloc(0x500);
  bytes.writeUInt16LE(0x5a4d, 0);
  bytes.writeUInt32LE(0x80, 0x3c);
  bytes.writeUInt32LE(0x4550, 0x80);
  bytes.writeUInt16LE(1, 0x86);
  const optionalSize = magic === 0x20b ? 240 : 224;
  bytes.writeUInt16LE(optionalSize, 0x94);
  bytes.writeUInt16LE(magic, 0x98);
  const directories = 0x98 + (magic === 0x20b ? 112 : 96);
  bytes.writeUInt32LE(16, directories - 4);
  const directory = directories + (delay ? 13 : 1) * 8;
  bytes.writeUInt32LE(0x1000, directory);
  bytes.writeUInt32LE(delay ? 64 : 40, directory + 4);
  const section = 0x98 + optionalSize;
  bytes.writeUInt32LE(0x1000, section + 12);
  bytes.writeUInt32LE(0x300, section + 16);
  bytes.writeUInt32LE(0x200, section + 20);
  if (delay) bytes.writeUInt32LE(1, 0x200);
  bytes.writeUInt32LE(0x1100, 0x200 + (delay ? 4 : 12));
  bytes.write(runtime + "\0", 0x300, "ascii");
  return bytes;
}

test("detects external runtime in PE32 and PE32+ normal imports", async () => {
  const { peImports, externalVcRuntimes } = await import("../lib/pe-imports.mjs");
  for (const magic of [0x10b, 0x20b]) {
    assert.deepEqual(externalVcRuntimes(peImports(peFile({ magic }))), ["VCRUNTIME140.dll"]);
  }
});

test("detects delayed and differently capitalized VC runtime imports", async () => {
  const { peImports, externalVcRuntimes } = await import("../lib/pe-imports.mjs");
  assert.deepEqual(externalVcRuntimes(peImports(peFile({ delay: true, runtime: "vcruntime140_1.dll" }))), ["vcruntime140_1.dll"]);
});

test("accepts Windows system UCRT dependencies", async () => {
  const { peImports, externalVcRuntimes } = await import("../lib/pe-imports.mjs");
  assert.deepEqual(externalVcRuntimes(peImports(peFile({ runtime: "api-ms-win-crt-runtime-l1-1-0.dll" }))), []);
});

test("rejects truncated and unmapped imports instead of passing the package", async () => {
  const { peImports } = await import("../lib/pe-imports.mjs");
  assert.throws(() => peImports(peFile().subarray(0, 128)), /截断/);
  const bytes = peFile();
  bytes.writeUInt32LE(0x9000, 0x20c);
  assert.throws(() => peImports(bytes), /未映射/);
});
