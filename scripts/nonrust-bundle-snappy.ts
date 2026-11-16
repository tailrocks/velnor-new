// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

"use strict";

export function decodeSnappy(input: Uint8Array): Uint8Array {
  const limit = 16 * 1024 * 1024;
  if (input.length > limit) throw new Error("compressed bundle exceeds limit");
  let cursor = 0;
  const byte = (): number => {
    const value = input[cursor++];
    if (value === undefined) throw new Error("truncated Snappy bundle");
    return value;
  };
  const littleEndian = (count: number): number => {
    let value = 0;
    for (let index = 0; index < count; index++) value += byte() * 2 ** (8 * index);
    return value;
  };
  let length = 0;
  for (let index = 0; index < 5; index++) {
    const value = byte();
    if (index === 4 && value > 15) throw new Error("invalid Snappy length");
    length += (value & 127) * 2 ** (7 * index);
    if (value < 128) break;
  }
  if (length > limit) throw new Error("decoded bundle exceeds limit");
  const output = new Uint8Array(length);
  let written = 0;
  while (cursor < input.length) {
    const tag = byte();
    const kind = tag & 3;
    if (kind === 0) {
      const upper = tag >>> 2;
      const count = upper < 60 ? upper + 1 : littleEndian(upper - 59) + 1;
      if (count > input.length - cursor || count > length - written) throw new Error("invalid Snappy literal");
      output.set(input.subarray(cursor, cursor + count), written);
      cursor += count;
      written += count;
    } else {
      const count = kind === 1 ? 4 + ((tag >>> 2) & 7) : 1 + (tag >>> 2);
      const offset = kind === 1 ? ((tag & 224) << 3) + byte() : littleEndian(kind === 2 ? 2 : 4);
      if (offset === 0 || offset > written || count > length - written) throw new Error("invalid Snappy copy");
      for (let index = 0; index < count; index++) {
        const value = output[written - offset];
        if (value === undefined) throw new Error("invalid Snappy source");
        output[written++] = value;
      }
    }
  }
  if (written !== length) throw new Error("incomplete Snappy bundle");
  return output;
}
