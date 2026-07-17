#!/usr/bin/env bash
# Generate a minimal icon-source.png using ImageMagick or Python fallback.
set -euo pipefail

if command -v convert &>/dev/null; then
  convert -size 1024x1024 xc:'#0E0F13' icon-source.png
  echo "Generated icon-source.png via ImageMagick"
else
  # Minimal 1x1 PNG that works as a valid source
  python3 -c "
import struct, zlib
w, h = 1024, 1024
raw = b''
for y in range(h):
    raw += b'\x00'
    for x in range(w):
        raw += b'\x0e\x0f\x13\xff'
def chk(typ, data):
    c = typ + data
    return struct.pack('>I', len(data)) + c + struct.pack('>I', zlib.crc32(c) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n'
png += chk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
png += chk(b'IDAT', zlib.compress(raw, level=1))
png += chk(b'IEND', b'')
with open('icon-source.png', 'wb') as f:
    f.write(png)
print('Generated icon-source.png via Python')
" 2>&1
fi
