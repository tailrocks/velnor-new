// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

interface Matcher {
  match(name: string): boolean;
}
type MatcherConstructor = new (pattern: string, options: Record<string, boolean>) => Matcher;
interface MinimatchModule {
  Minimatch: MatcherConstructor;
}

const OPTIONS = { dot: true, nobrace: true, nocase: false, nocomment: true, noext: true, nonegate: true };
const PATTERN_COUNT = 2600;
const NAMES_PER_PATTERN = 12;
const ENTRIES_PER_LINE = 10;
const REFUSED = "refused";
const NOT_INERT = /[()[{*+?|^$\\]/;

const HANDWRITTEN = [
  "[a-c-e]é", "*[a\\-c]*", "[\\]]?", "*[!]", "[a-c-e]*", "[!]", "[^]", "[]", "[]]", "[]a]", "[](]", "[]**]", "[]\\]]",
  "[!a]", "[!a-c]", "[^a-c]", "[a^]", "[!^]", "[\\!a]", "[a\\-]", "[\\-a]", "[a-]", "[-a]", "[--a]", "[a--]", "[a-c-e-g]",
  "[z-a]", "[z\\-a]", "[a-\\]]", "[\\]-a]", "[\\d]", "[\\d-z]", "[a-\\w]", "[\\x41-\\x30]", "[\\x30-\\x41]", "[\\u0041-\\u0030]",
  "[\\cA-\\cB]", "[\\cB-\\cA]", "[\\101-\\60]", "[\\0-\\1]", "[\\b-\\t]", "[é-ü]", "[ü-é]", "[😀]", "[😀-😁]", "?", "??", "???", "?😀",
  "😀?", "[!a]?", "a[", "a[b", "[a", "[a-", "[\\", "a\\", "\\a", "\\\\", "\\\\a", "a\\\\", "\\*", "\\?", "\\[", "\\]", "\\@", "\\@\\*",
  "\\@a", "@", "a@b", "!", "!a", "a!", "+", "a+b", "@(a|b)", "+(a)", "!(a)", "?(a)", "*(a)", "(", ")", "a(b", "a|b", "{a,b}", "a{b", "x{1}",
  "$", "a$", "^a", "a^", "#a", "~", "a.b", ".a", ".*", "*.a", "*", "**", "***", "a**b", "a*", "*a", "*a*", "a*b*c", "*?", "?*", "*[a]*",
  "[a]*", "*[a]", "a[*]", "[*]", "[?]", "[(]", "[)]", "[|]", "[@]", "[+]", "[.]", "[$]", "[{]", "[}]", "[\\[]", "[a[]", "[[]", "[[a]",
  "[a[b]", "[a[b]c]", "[z-a[b]", "[z-a[bc]", "*[z-a]", "*[z-a[", "a*[", "*[", "*[**", "x[**", "*x[**", "*[]", "*[]a", "[a]b]", "[a]]",
  "[\\u0030-\\u0041]", "[\\cA-\\cZ]", "[\\cZ-\\cA]", "[\\060-\\101]", "[\\101-\\060]", "[\\8-\\9]", "[\\t-\\r]", "[\\r-\\t]",
  "[\\c1-\\c_]", "[\\c-]", "[\\x]", "[\\x4-a]", "[\\u00-a]", "[\\d-]", "[\\D-a]", "[\\s-\\w]", "[a\\d-z]", "a\\\u2028b", "a\\\nb", "a\\\rb",
  "[\\c-a]", "[\\b]", "[\\b-\\t]", "[z-a*]", "[z-a?]", "*[z-a]", "*[z-a]x",
  "é", "é*", "*é", "?é", "é?", "[é]", "[!é]", "[é]?", "[a-é]", "[a-c-é]", "\\é", "\\😀", "[\\😀]", "*😀*",
];

const PATTERN_BAG = ["a", "a", "b", "c", "e", "é", "-", "-", "!", "^", "[", "[", "]", "]", "\\", "\\", "*", "*", "?", "?", "@", "x", "😀", ".", "{", "(", ")", "|", "$", "+", "1"];
const NAME_BAG = ["a", "b", "c", "e", "é", "x", "-", "!", "^", "[", "]", "\\", "*", "?", "@", "😀", ".", "1", "\n", " ", "d", "ü", "A", "{", "("];
const FIXED_NAMES = ["x[undefined", "[undefined", "a", "é", "😀", "\n", "\nx", "x\n", " a", "ab", "a-c", "-", "]", "[", "!", "^", "\\", "é-", "a.b", ".a"];

function generator(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

function loadMinimatch(directory: string): MatcherConstructor {
  const require = createRequire(join(resolve(directory), "package.json"));
  const loaded: unknown = require("minimatch");
  if (typeof loaded !== "function" || !("Minimatch" in loaded)) throw new Error("minimatch did not load");
  return loaded.Minimatch as MatcherConstructor;
}

function refusedProbe(Minimatch: MatcherConstructor, pattern: string): boolean {
  const native = globalThis.RegExp;
  let refused = false;
  globalThis.RegExp = new Proxy(native, {
    apply(target, receiver, args: [string, string?]) {
      const [source] = args;
      if (source.startsWith("[]") && source.endsWith("]") && NOT_INERT.test(source.slice(2, -1))) refused = true;
      return Reflect.apply(target, receiver, args);
    },
  });
  try {
    new Minimatch(pattern, OPTIONS);
  } finally {
    globalThis.RegExp = native;
  }
  return refused;
}

function validName(name: string): boolean {
  return name !== "" && name !== "." && name !== ".." && !name.includes("/") && !name.includes("\0");
}

function validPattern(pattern: string): boolean {
  return pattern !== "" && !pattern.includes("/") && !pattern.includes("\0") && !/\s/u.test(pattern);
}

function pick<T>(random: () => number, bag: readonly T[]): T {
  return bag[Math.floor(random() * bag.length)] as T;
}

function randomText(random: () => number, bag: readonly string[], minimum: number, maximum: number): string {
  const length = minimum + Math.floor(random() * (maximum - minimum + 1));
  return Array.from({ length }, () => pick(random, bag)).join("");
}

function instantiate(random: () => number, pattern: string): string {
  const characters = Array.from(pattern);
  let out = "";
  for (let index = 0; index < characters.length; index++) {
    const character = characters[index] as string;
    if (character === "*") out += randomText(random, NAME_BAG, 0, 3);
    else if (character === "?") out += pick(random, NAME_BAG);
    else if (character === "\\" && index + 1 < characters.length) out += characters[++index];
    else if (character === "[") {
      const end = characters.indexOf("]", index + 1);
      const inside = end < 0 ? [] : characters.slice(index + 1, end);
      out += inside.length > 0 && random() < 0.8 ? pick(random, inside) : pick(random, NAME_BAG);
      if (end > 0) index = end;
    } else out += character;
  }
  return random() < 0.3 ? out + pick(random, NAME_BAG) : out;
}

function namesFor(random: () => number, Minimatch: MatcherConstructor, pattern: string): string[] {
  const matcher = new Minimatch(pattern, OPTIONS);
  const candidates = new Set<string>(FIXED_NAMES);
  candidates.add(pattern);
  candidates.add(pattern.replace(/\\(.)/g, "$1"));
  candidates.add(pattern.replaceAll("*", "b").replaceAll("?", "b"));
  for (let attempt = 0; attempt < 120; attempt++) {
    candidates.add(instantiate(random, pattern));
    candidates.add(randomText(random, NAME_BAG, 1, 5));
  }
  const pool = [...candidates].filter(validName);
  const matching = pool.filter((name) => matcher.match(name));
  const rejecting = pool.filter((name) => !matcher.match(name));
  const half = NAMES_PER_PATTERN / 2;
  const chosen = new Set<string>();
  for (const name of matching.slice(0, half)) chosen.add(name);
  for (const name of rejecting.slice(0, half)) chosen.add(name);
  for (const name of pool) {
    if (chosen.size >= NAMES_PER_PATTERN) break;
    chosen.add(name);
  }
  return [...chosen];
}

function patterns(random: () => number, count: number): string[] {
  const unique = new Set<string>(HANDWRITTEN);
  while (unique.size < count) {
    const pattern = randomText(random, PATTERN_BAG, 1, 8);
    if (validPattern(pattern)) unique.add(pattern);
  }
  return [...unique];
}

function entry(random: () => number, Minimatch: MatcherConstructor, pattern: string): string[] {
  const names = namesFor(random, Minimatch, pattern);
  if (refusedProbe(Minimatch, pattern)) return [pattern, REFUSED, ...names];
  const matcher = new Minimatch(pattern, OPTIONS);
  const bits = names.map((name) => (matcher.match(name) ? "1" : "0")).join("");
  return [pattern, bits, ...names];
}

const [minimatchDirectory, output, requested] = process.argv.slice(2);
if (minimatchDirectory === undefined || output === undefined) {
  throw new Error("usage: bun scripts/minimatch-corpus.ts <directory holding node_modules/minimatch@3.1.5> <output.json> [pattern count]");
}
const Minimatch = loadMinimatch(minimatchDirectory);
const random = generator(0x2892);
const entries = patterns(random, requested === undefined ? PATTERN_COUNT : Number(requested)).map((pattern) => JSON.stringify(entry(random, Minimatch, pattern)));
const lines: string[] = [];
for (let start = 0; start < entries.length; start += ENTRIES_PER_LINE) {
  lines.push(entries.slice(start, start + ENTRIES_PER_LINE).join(","));
}
await Bun.write(output, `[\n${lines.join(",\n")}\n]\n`);
const cases = entries.reduce((total, line) => total + (JSON.parse(line) as string[]).length - 2, 0);
console.log(`${entries.length} patterns, ${cases} cases, ${lines.length + 2} lines`);
