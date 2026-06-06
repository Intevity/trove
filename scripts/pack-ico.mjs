#!/usr/bin/env node
// Packs pre-rendered PNG frames into a single .ico container.
//
// Usage: node scripts/pack-ico.mjs <out.ico> <frame.png> [frame.png ...]
//
// Every frame is stored PNG-compressed. Windows Vista+ renders PNG frames
// at any size, and the previously shipped (tauri-cli-generated) icon.ico
// already used PNG encoding for all frames — we keep that layout because
// ImageMagick's BMP-in-ico writer mangles the alpha channel (see
// scripts/generate-icons.sh).

import { readFileSync, writeFileSync } from 'node:fs';

const [out, ...framePaths] = process.argv.slice(2);
if (!out || framePaths.length === 0) {
  console.error('usage: pack-ico.mjs <out.ico> <frame.png> [frame.png ...]');
  process.exit(1);
}

const pngSize = (buf) => {
  // PNG signature is 8 bytes; IHDR follows with width/height at offsets 16/20.
  if (buf.readUInt32BE(0) !== 0x89504e47) {
    throw new Error('not a PNG file');
  }
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
};

const frames = framePaths.map((p) => {
  const data = readFileSync(p);
  const { width, height } = pngSize(data);
  if (width > 256 || height > 256) {
    throw new Error(`${p}: ${width}x${height} exceeds the 256px ico maximum`);
  }
  return { data, width, height };
});

// ICONDIR (6 bytes) + one ICONDIRENTRY (16 bytes) per frame, then the blobs.
const header = Buffer.alloc(6 + 16 * frames.length);
header.writeUInt16LE(0, 0); // reserved
header.writeUInt16LE(1, 2); // type: icon
header.writeUInt16LE(frames.length, 4);

let offset = header.length;
frames.forEach((f, i) => {
  const entry = 6 + 16 * i;
  header.writeUInt8(f.width === 256 ? 0 : f.width, entry); // 0 encodes 256
  header.writeUInt8(f.height === 256 ? 0 : f.height, entry + 1);
  header.writeUInt8(0, entry + 2); // palette colors: none
  header.writeUInt8(0, entry + 3); // reserved
  header.writeUInt16LE(1, entry + 4); // color planes
  header.writeUInt16LE(32, entry + 6); // bits per pixel
  header.writeUInt32LE(f.data.length, entry + 8);
  header.writeUInt32LE(offset, entry + 12);
  offset += f.data.length;
});

writeFileSync(out, Buffer.concat([header, ...frames.map((f) => f.data)]));
console.log(`${out}: ${frames.length} PNG frames`);
