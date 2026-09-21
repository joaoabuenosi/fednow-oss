#!/usr/bin/env node
/**
 * make-test-png.mjs — emit a solid-colour PNG of a given size.
 *
 * Used only by scripts/selftest-checks.sh, to produce the two shapes of bad
 * social-preview image that check [8/9] must reject: the wrong dimensions, and
 * a correctly-sized image whose bottom band is one flat colour (what a render
 * cropped by Chromium's "new" headless mode looks like). Generating them here
 * rather than shipping fixtures keeps a browser out of `npm run check`.
 *
 * Usage: node make-test-png.mjs <out.png> <width> <height>
 */
import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';
import { crc32 } from 'node:zlib';

const [out, w, h] = [process.argv[2], Number(process.argv[3]), Number(process.argv[4])];
if (!out || !w || !h) {
  console.error('usage: make-test-png.mjs <out.png> <width> <height>');
  process.exit(2);
}

const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body) >>> 0);
  return Buffer.concat([len, body, crc]);
};

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(w, 0);
ihdr.writeUInt32BE(h, 4);
ihdr[8] = 8;   // bit depth
ihdr[9] = 2;   // colour type: truecolour RGB
// 10..12 = compression, filter, interlace: all 0

// One flat colour, filter type 0 on every scanline.
const stride = w * 3;
const raw = Buffer.alloc(h * (stride + 1));
for (let y = 0; y < h; y++) {
  const row = y * (stride + 1);
  raw[row] = 0;
  raw.fill(0x11, row + 1, row + 1 + stride);
}

writeFileSync(out, Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw)),
  chunk('IEND', Buffer.alloc(0)),
]));
