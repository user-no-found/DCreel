import fs from "node:fs/promises";
import path from "node:path";
import sharp from "sharp";

const projectRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(projectRoot, "Creel .png");
const iconDirectory = path.join(projectRoot, "src-tauri", "icons");
const publicDirectory = path.join(projectRoot, "public");
const masterPath = path.join(iconDirectory, "creel-icon.png");
const publicPath = path.join(publicDirectory, "creel-icon.png");

await fs.mkdir(iconDirectory, { recursive: true });
await fs.mkdir(publicDirectory, { recursive: true });

const { data, info } = await sharp(sourcePath)
  .ensureAlpha()
  .raw()
  .toBuffer({ resolveWithObject: true });

const { width, height, channels } = info;
if (channels !== 4) throw new Error(`Expected RGBA source data, got ${channels} channels`);

// The supplied illustration is a light rounded square on a black square canvas.
// Flood only dark pixels connected to the canvas edge so the black ink inside the
// illustration remains untouched. Downsampling later supplies a clean antialiased edge.
const visited = new Uint8Array(width * height);
const queue = new Int32Array(width * height);
let head = 0;
let tail = 0;
const threshold = 224;

function isExteriorCandidate(pixelIndex) {
  const offset = pixelIndex * channels;
  const luminance =
    data[offset] * 0.2126 + data[offset + 1] * 0.7152 + data[offset + 2] * 0.0722;
  return luminance <= threshold;
}

function enqueue(pixelIndex) {
  if (visited[pixelIndex] || !isExteriorCandidate(pixelIndex)) return;
  visited[pixelIndex] = 1;
  queue[tail++] = pixelIndex;
}

for (let x = 0; x < width; x += 1) {
  enqueue(x);
  enqueue((height - 1) * width + x);
}
for (let y = 1; y < height - 1; y += 1) {
  enqueue(y * width);
  enqueue(y * width + width - 1);
}

while (head < tail) {
  const pixel = queue[head++];
  const x = pixel % width;
  const y = Math.floor(pixel / width);
  if (x > 0) enqueue(pixel - 1);
  if (x + 1 < width) enqueue(pixel + 1);
  if (y > 0) enqueue(pixel - width);
  if (y + 1 < height) enqueue(pixel + width);
}

for (let pixel = 0; pixel < visited.length; pixel += 1) {
  if (visited[pixel]) data[pixel * channels + 3] = 0;
}

await sharp(data, { raw: info })
  .resize(1024, 1024, { fit: "fill", kernel: sharp.kernel.lanczos3 })
  .png({ compressionLevel: 9 })
  .toFile(masterPath);

await sharp(masterPath)
  .resize(256, 256, { fit: "contain" })
  .png({ compressionLevel: 9 })
  .toFile(publicPath);

console.log(`Removed ${tail.toLocaleString()} edge-connected pixels.`);
console.log(`Wrote ${path.relative(projectRoot, masterPath)} and ${path.relative(projectRoot, publicPath)}.`);
