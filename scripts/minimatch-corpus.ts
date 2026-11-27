// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

interface Matcher {
  match(name: string): boolean;
}
type MatcherConstructor = new (pattern: string, options: Record<string, boolean>) => Matcher;
interface MinimatchModule {
  Minimatch: MatcherConstructor;
}

const MINIMATCH_VERSION = "3.1.2";
const OPTIONS = { dot: true, nobrace: true, nocase: false, nocomment: true, noext: true, nonegate: true };
const PATTERN_COUNT = 2600;
const NAMES_PER_PATTERN = 20;
const SAMPLED_BOUNDARY_NAMES = 4;
const LONG_PREFIX_TAIL = 3;
const ENTRIES_PER_LINE = 10;
const DEFAULT_SEED = 0x2892;
const PATH_PREFIX = "/r/";
const PATH_SUFFIX = "/z";
const MAX_CODE_POINT = 0x10ffff;
const SURROGATES = [0xd800, 0xdfff] as const;
const REFUSED = "refused";
const OPAQUE_GROUP = /^(?:<[^=!]|[ims-])/;

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
  "foo[*", "x[?", "[😀?", "foo[", "[*", "[?", "x[*y", "[a*", "[a?b", "[*[", "[*]", "[**", "x[**y", "[\\*", " ", "  ", "\t", " a", "a ", " a ", "a b", "[ ]", "[ a]", "* ", " *", "\u2028", "a\u2028", "[\u2028]", "[^\n]", "\r", "*\n*", "\u00a0", "\ufeff*", "[ -~]",
  "[a\\@])]", "[\\@]z-a]", "[\\@]]", "[\\@]]]", "[\\@](]", "[\\@]()]", "[\\@](a)]", "[\\@](?:a)]", "[\\@](?=a)*]", "[\\@](?<=a)*]", "[\\@](?<=a)]",
  "[\\@]*]", "[\\@]+]", "[\\@]?]", "[\\@]??]", "[\\@]*?]", "[\\@]*??]", "[\\@]???]", "[\\@]{1}??]", "[\\@]**]", "[\\@]{1}]", "[\\@]{2,1}]", "[\\@]{,}]", "[\\@]{1,2}?]", "[\\@]{]", "[\\@]}]",
  "[\\@]^*]", "[\\@]$?]", "[\\@]\\b*]", "[\\@]\\b]", "[\\@]|*]", "[\\@]|]", "[\\@]\\]]", "[\\@][z-a]]", "[\\@][a]]", "[\\@\\@]x]", "[\\@@]x]", "[\\\\@]x]",
  "[\\@](?x)]", "[\\@](?<é>a)]", "[\\@]{1,}]", "[\\@]a{1,1}]", "[\\@]a{2147483648,2147483647}]", "[\\@]a{1,99999999999999999999}]", "[\\@]a{99999999999999999999,1}]", "[\\@]\\k]", "[\\@]\\k<a>]", "[\\@](?<n>a)]", "[\\@](?<1>a)]", "[\\@](?i:a)]", "[\\@](?ii:a)]", "[\\@](?-:a)]", "[a\\@]", "[a\\@]*", "*[a\\@]b]", "[\\@é]é]", "[\\@😀]😀]",
  "[]{1}]", "[]a|b]", "[](?:)]", "[](?!a)+]", "[]\\b+]", "[]\\]]", "[](?<n>a)]", "[]a{2,1}]", "[]*]", "[]|*]", "[]x{1,2}{3}]",
  "é", "é*", "*é", "?é", "é?", "[é]", "[!é]", "[é]?", "[a-é]", "[a-c-é]", "\\é", "\\😀", "[\\😀]", "*😀*",
];

const PATTERN_BAG = ["a", "a", "b", "c", "e", "é", "-", "-", "!", "^", "[", "[", "]", "]", "\\", "\\", "\\@", "\\@]", "*", "*", "?", "?", "@", "x", "😀", ".", "{", "(", ")", "|", "$", "+", "<", "1", " ", "\n", "\u2028"];
const NAME_BAG = ["a", "b", "c", "e", "é", "x", "-", "!", "^", "[", "]", "\\", "*", "?", "@", "😀", ".", "1", "\n", " ", "d", "ü", "A", "{", "("];
const BOUNDARY_NAMES = [
  "\u0001", "\u0008", "\t", "\n", "\u000b", "\u000c", "\r", "\u001f", " ", "\u007f", "\u0080", "\u00a0", "\u2027", "\u2028", "\u2029", "\u202a",
  "\ud7ff", "\ue000", "\ufeff", "\uffff", "\u{10000}", "\u{10ffff}", "a\u2028", "\u2028a", "a\n", "\na", "a\r", "\ra",
];
const FIXED_NAMES = ["x[undefined", "[undefined", "a", "é", "😀", "\n", "\nx", "x\n", " a", "ab", "a-c", "-", "]", "[", "!", "^", "\\", "é-", "a.b", ".a"];

const FORMAT_CHARACTER = /\p{Cf}/gu;

function escapeFormatCharacters(json: string): string {
  return json.replace(FORMAT_CHARACTER, (character) =>
    Array.from({ length: character.length }, (_, index) => `\\u${character.charCodeAt(index).toString(16).padStart(4, "0")}`).join(""),
  );
}

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
  const root = resolve(directory);
  const installed = JSON.parse(readFileSync(join(root, "node_modules", "minimatch", "package.json"), "utf8")) as { version?: string };
  if (installed.version !== MINIMATCH_VERSION) {
    throw new Error(`expected minimatch@${MINIMATCH_VERSION} (the version bundled in actions/runner v2.338.2), found ${installed.version ?? "none"}`);
  }
  const require = createRequire(join(root, "package.json"));
  const loaded: unknown = require("minimatch");
  if (typeof loaded !== "function" || !("Minimatch" in loaded)) throw new Error("minimatch did not load");
  return loaded.Minimatch as MatcherConstructor;
}

function holdsOpaqueGroup(source: string): boolean {
  for (let index = 0; index < source.length; index++) {
    const character = source[index];
    if (character === "\\") index++;
    else if (character === "[") {
      for (index++; index < source.length && source[index] !== "]"; index++) if (source[index] === "\\") index++;
    } else if (character === "(" && source[index + 1] === "?" && OPAQUE_GROUP.test(source.slice(index + 2, index + 4))) return true;
  }
  return false;
}

function refusedProbe(Minimatch: MatcherConstructor, pattern: string): boolean {
  const native = globalThis.RegExp;
  let refused = false;
  globalThis.RegExp = new Proxy(native, {
    apply(target, receiver, args: [string, string?]) {
      const compiled: unknown = Reflect.apply(target, receiver, args);
      const [source] = args;
      if (source.startsWith("[") && source.endsWith("]") && holdsOpaqueGroup(source)) refused = true;
      return compiled;
    },
  });
  try {
    new Minimatch(embedded(pattern), OPTIONS);
  } finally {
    globalThis.RegExp = native;
  }
  return refused;
}

function validName(name: string): boolean {
  return name !== "" && name !== "." && name !== ".." && !name.includes("/") && !name.includes("\0");
}

function validPattern(pattern: string): boolean {
  return pattern !== "" && !pattern.includes("/") && !pattern.includes("\0");
}

function embedded(text: string): string {
  return `${PATH_PREFIX}${text}${PATH_SUFFIX}`;
}

function compile(Minimatch: MatcherConstructor, pattern: string): (name: string) => boolean {
  const matcher = new Minimatch(embedded(pattern), OPTIONS);
  return (name) => matcher.match(embedded(name));
}

function neighbours(pattern: string): string[] {
  const found: string[] = [];
  for (const character of new Set(Array.from(pattern))) {
    const code = character.codePointAt(0) as number;
    for (const next of [code - 1, code + 1]) {
      const surrogate = next >= SURROGATES[0] && next <= SURROGATES[1];
      if (next >= 0 && next <= MAX_CODE_POINT && !surrogate) found.push(String.fromCodePoint(next));
    }
  }
  return found;
}

function prefixes(text: string): string[] {
  const characters = Array.from(text);
  return characters.map((_, index) => characters.slice(0, index + 1).join(""));
}

function longPrefixes(text: string): string[] {
  const all = prefixes(text);
  return all.slice(Math.max(0, all.length - LONG_PREFIX_TAIL - 1), all.length - 1);
}

function boundaryNames(random: () => number, pattern: string): string[] {
  const plain = pattern.replace(/\\(.)/gsu, "$1");
  const tails = [...longPrefixes(plain), ...longPrefixes(plain.replaceAll("*", "b").replaceAll("?", "b"))].filter(validName);
  const pool = [...new Set([...neighbours(pattern), ...BOUNDARY_NAMES])].filter(validName);
  for (let index = pool.length - 1; index > 0; index--) {
    const other = Math.floor(random() * (index + 1));
    [pool[index], pool[other]] = [pool[other] as string, pool[index] as string];
  }
  return [...new Set([...tails, ...pool.slice(0, SAMPLED_BOUNDARY_NAMES)])];
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
  const matches = compile(Minimatch, pattern);
  const candidates = new Set<string>(FIXED_NAMES);
  candidates.add(pattern);
  candidates.add(pattern.replace(/\\(.)/g, "$1"));
  candidates.add(pattern.replaceAll("*", "b").replaceAll("?", "b"));
  for (let attempt = 0; attempt < 120; attempt++) {
    candidates.add(instantiate(random, pattern));
    candidates.add(randomText(random, NAME_BAG, 1, 5));
  }
  const pool = [...candidates].filter(validName);
  const matching = pool.filter((name) => matches(name));
  const rejecting = pool.filter((name) => !matches(name));
  const chosen = new Set<string>(boundaryNames(random, pattern));
  const half = Math.ceil((NAMES_PER_PATTERN - chosen.size) / 2);
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
  const matches = compile(Minimatch, pattern);
  const bits = names.map((name) => (matches(name) ? "1" : "0")).join("");
  return [pattern, bits, ...names];
}

const [minimatchDirectory, output, requested, seed] = process.argv.slice(2);
if (minimatchDirectory === undefined || output === undefined) {
  throw new Error("usage: node (24.22.1, the V8 of the shipped runner) scripts/minimatch-corpus.ts <directory holding node_modules/minimatch@3.1.2, the version bundled in actions/runner v2.338.2> <output.json> [pattern count] [seed]");
}
const Minimatch = loadMinimatch(minimatchDirectory);
const random = generator(seed === undefined ? DEFAULT_SEED : Number(seed));
const entries = patterns(random, requested === undefined ? PATTERN_COUNT : Number(requested)).map((pattern) => escapeFormatCharacters(JSON.stringify(entry(random, Minimatch, pattern))));
const lines: string[] = [];
for (let start = 0; start < entries.length; start += ENTRIES_PER_LINE) {
  lines.push(entries.slice(start, start + ENTRIES_PER_LINE).join(","));
}
writeFileSync(output, `[\n${lines.join(",\n")}\n]\n`);
const cases = entries.reduce((total, line) => total + (JSON.parse(line) as string[]).length - 2, 0);
console.log(`${entries.length} patterns, ${cases} cases, ${lines.length + 2} lines`);
