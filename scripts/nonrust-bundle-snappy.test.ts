// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

"use strict";

import { expect, test } from "bun:test";
import { decodeSnappy } from "./nonrust-bundle-snappy";

for (const [name, bytes, expected] of [
  ["empty", [0], ""],
  ["consecutive literals", [2, 0, 97, 0, 98], "ab"],
  ["copy1 overlap", [7, 8, 120, 97, 98, 1, 2], "xababab"],
  ["copy2 overlap", [7, 8, 120, 97, 98, 14, 2, 0], "xababab"],
  ["copy4 overlap", [7, 8, 120, 97, 98, 15, 2, 0, 0, 0], "xababab"],
  ["offset1 overlap", [8, 0, 97, 26, 1, 0], "aaaaaaaa"],
  ["literal width1", [1, 240, 0, 97], "a"],
  ["literal width2", [1, 244, 0, 0, 97], "a"],
  ["literal width3", [1, 248, 0, 0, 0, 97], "a"],
  ["literal width4", [1, 252, 0, 0, 0, 0, 97], "a"],
] satisfies [string, number[], string][]) {
  test(name, () => expect(new TextDecoder().decode(decodeSnappy(Uint8Array.from(bytes)))).toBe(expected));
}

for (const [name, bytes] of [
  ["missing varint", []],
  ["truncated varint", [128]],
  ["fifth varint high bits", [128, 128, 128, 128, 16]],
  ["continued fifth varint", [128, 128, 128, 128, 128]],
  ["output over cap", [129, 128, 128, 8]],
  ["incomplete output", [1]],
  ["trailing input", [0, 0, 97]],
  ["literal overrun", [1, 4, 97, 98]],
  ["truncated literal", [1, 0]],
  ["truncated length1", [1, 240]],
  ["truncated length2", [1, 244, 0]],
  ["truncated length3", [1, 248, 0, 0]],
  ["truncated length4", [1, 252, 0, 0, 0]],
  ["huge literal", [1, 252, 255, 255, 255, 255]],
  ["copy before output", [4, 1, 1]],
  ["zero offset", [5, 0, 97, 1, 0]],
  ["backward overflow", [5, 0, 97, 1, 2]],
  ["copy output overrun", [2, 0, 97, 1, 1]],
  ["truncated offset1", [5, 0, 97, 1]],
  ["truncated offset2", [5, 0, 97, 14, 1]],
  ["truncated offset4", [5, 0, 97, 15, 1, 0, 0]],
  ["copy4 unsigned offset", [5, 0, 97, 15, 0, 0, 0, 128]],
] satisfies [string, number[]][]) {
  test(name, () => expect(() => decodeSnappy(Uint8Array.from(bytes))).toThrow());
}

test("decoded 16MiB boundary", () => {
  const bytes = new Uint8Array(4 + 2 + 262_143 * 3 + 3);
  bytes.set([128, 128, 128, 8, 0, 97]);
  for (let index = 6; index < bytes.length - 3; index += 3) bytes.set([254, 1, 0], index);
  bytes.set([250, 1, 0], bytes.length - 3);
  const decoded = decodeSnappy(bytes);
  expect(decoded.length).toBe(16 * 1024 * 1024);
  expect(decoded.every(value => value === 97)).toBe(true);
});

test("compressed 16MiB boundary", () => {
  const bytes = new Uint8Array(16 * 1024 * 1024);
  bytes.set([254, 255, 255, 3]);
  for (let index = 4; index < bytes.length; index += 2) bytes.set([0, 97], index);
  expect(decodeSnappy(bytes).length).toBe(8_388_606);
});

for (const [name, suffix] of [
  ["copy1 high offset", [33, 0]],
  ["copy2 high offset", [14, 0, 1]],
  ["copy4 high offset", [15, 0, 1, 0, 0]],
] satisfies [string, number[]][]) {
  test(name, () => {
    const bytes = Uint8Array.from([132, 2, 240, 255, ...new Uint8Array(256).fill(97), ...suffix]);
    expect(new TextDecoder().decode(decodeSnappy(bytes))).toBe("a".repeat(260));
  });
}

for (const [name, prefix, length] of [
  ["literal61", [61, 240, 60], 61],
  ["literal300", [172, 2, 244, 43, 1], 300],
  ["literal66000 width3", [208, 131, 4, 248, 207, 1, 1], 66_000],
  ["literal66000 width4", [208, 131, 4, 252, 207, 1, 1, 0], 66_000],
] satisfies [string, number[], number][]) {
  test(name, () => {
    const bytes = new Uint8Array(prefix.length + length).fill(97);
    bytes.set(prefix);
    expect(new TextDecoder().decode(decodeSnappy(bytes))).toBe("a".repeat(length));
  });
}

test("valid expansion over output cap", () => {
  const bytes = new Uint8Array(6 + 262_144 * 3);
  bytes.set([129, 128, 128, 8, 0, 97]);
  for (let index = 6; index < bytes.length; index += 3) bytes.set([254, 1, 0], index);
  expect(() => decodeSnappy(bytes)).toThrow("decoded bundle exceeds limit");
});

test("valid stream over input cap", () => {
  const bytes = new Uint8Array(16 * 1024 * 1024 + 1);
  bytes.set([254, 255, 255, 3]);
  for (let index = 4; index < bytes.length - 3; index += 2) bytes.set([0, 97], index);
  bytes.set([240, 0, 97], bytes.length - 3);
  expect(() => decodeSnappy(bytes)).toThrow("compressed bundle exceeds limit");
});
