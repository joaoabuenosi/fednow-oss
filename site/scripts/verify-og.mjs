#!/usr/bin/env node
/**
 * verify-og.mjs — assert that public/og-image.png is a usable social preview.
 *
 * Two failure modes are worth catching mechanically, because neither is visible
 * in a diff and a wrong social card is only ever noticed by the person you were
 * trying to impress:
 *
 *   1. Wrong dimensions. Open Graph and Twitter/X want 1200x630 for a large
 *      summary card; anything else is letterboxed or refused.
 *   2. A CROPPED render. Chromium's "new" headless mode lays a page out in a
 *      viewport shorter than --window-size, so a fixed-size screenshot can come
 *      back the right size with the bottom of the artwork simply not painted —
 *      a flat band of background colour where the frame's bottom edge should
 *      be. This checks that the bottom rows actually contain artwork.
 *
 * Run by scripts/render-og.sh, and again by scripts/check-build.sh so a bad PNG
 * cannot be committed and forgotten.
 */
import { readFileSync } from 'node:fs';
import { inflateSync } from 'node:zlib';

const file = process.argv[2];
if (!file) {
  console.error('usage: node verify-og.mjs <png>');
  process.exit(2);
}

const EXPECTED_WIDTH = 1200;
const EXPECTED_HEIGHT = 630;

const png = readFileSync(file);
if (png.readUInt32BE(0) !== 0x89504e47) fail('not a PNG file.');

const width = png.readUInt32BE(16);
const height = png.readUInt32BE(20);
const bitDepth = png[24];
const colorType = png[25];
const interlace = png[28];

if (width !== EXPECTED_WIDTH || height !== EXPECTED_HEIGHT) {
  fail(`is ${width}x${height}; social previews must be ${EXPECTED_WIDTH}x${EXPECTED_HEIGHT}.`);
}
if (bitDepth !== 8 || (colorType !== 2 && colorType !== 6) || interlace !== 0) {
  // Only the shapes Chromium emits are decoded here; anything else is a signal
  // the file was produced by a different tool than render-og.sh.
  fail(`has an unexpected encoding (bit depth ${bitDepth}, colour type ${colorType}, interlace ${interlace}).`);
}

// Concatenate IDAT chunks and inflate.
const idat = [];
for (let off = 8; off + 8 <= png.length; ) {
  const len = png.readUInt32BE(off);
  const type = png.toString('ascii', off + 4, off + 8);
  if (type === 'IDAT') idat.push(png.subarray(off + 8, off + 8 + len));
  if (type === 'IEND') break;
  off += 12 + len;
}
const raw = inflateSync(Buffer.concat(idat));

// Undo the per-scanline filters. Straight from the PNG spec; nothing clever.
const channels = colorType === 6 ? 4 : 3;
const stride = width * channels;
const pixels = Buffer.alloc(height * stride);
for (let y = 0, pos = 0; y < height; y++) {
  const filter = raw[pos++];
  const line = raw.subarray(pos, pos + stride);
  pos += stride;
  const out = pixels.subarray(y * stride, (y + 1) * stride);
  const prev = y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : null;
  for (let x = 0; x < stride; x++) {
    const a = x >= channels ? out[x - channels] : 0;
    const b = prev ? prev[x] : 0;
    const c = prev && x >= channels ? prev[x - channels] : 0;
    let value = line[x];
    if (filter === 1) value += a;
    else if (filter === 2) value += b;
    else if (filter === 3) value += (a + b) >> 1;
    else if (filter === 4) {
      const p = a + b - c;
      const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
      value += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
    } else if (filter !== 0) fail(`uses unknown scanline filter ${filter} on row ${y}.`);
    out[x] = value & 0xff;
  }
}

/** Count distinct colours in a horizontal band — artwork varies, a crop does not. */
function distinctColours(fromRow, toRow) {
  const seen = new Set();
  for (let y = fromRow; y < toRow; y++) {
    for (let x = 0; x < width; x += 3) {
      const i = y * stride + x * channels;
      seen.add((pixels[i] << 16) | (pixels[i + 1] << 8) | pixels[i + 2]);
    }
  }
  return seen.size;
}

// The frame's bottom edge and the ember gradient both reach into the last 40
// rows, so a correct render has hundreds of colours there. A cropped one has
// exactly one: the flat page background the screenshot padded it out with.
const BAND = 40;
const bottom = distinctColours(height - BAND, height);
if (bottom < 8) {
  fail(`has a flat ${bottom}-colour band across its bottom ${BAND}px — the render was cropped. ` +
       'Re-run scripts/render-og.sh with a Chromium that honours --window-size (Playwright\'s headless_shell does).');
}

console.log(`verify-og: ${file} is ${width}x${height}, artwork reaches the bottom edge (${bottom} colours in the last ${BAND}px).`);

function fail(message) {
  console.error(`verify-og: FAIL — ${file} ${message}`);
  process.exit(1);
}
