// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";

import { expect, test } from "bun:test";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
const project = new URL("../../../", import.meta.url);
const sourcePath = new URL("../src/homebrew_qualification_acquire.ts", import.meta.url);
const fixture = new URL("homebrew_qualification/api_fixture.ts", import.meta.url).pathname;
for (const mode of ["complete", "changed-main", "oversized", "foreign-bundle"]) {
  test(`bounded historical acquisition ${mode}`, async () => {
    const temporary = await mkdtemp(join(tmpdir(), "homebrew-acquisition-test-"));
    try {
      const a = await readFile(new URL(".velnor/qualification/homebrew/preview-a.json", project), "utf8");
      const d = await readFile(new URL(".velnor/qualification/homebrew/preview-d.json", project), "utf8");
      const source = (await readFile(sourcePath, "utf8")).replace('"__VELNOR_HOMEBREW_CATALOG__"', JSON.stringify(`[${a},${d}]`));
      const script = join(temporary, "acquire.ts");
      const evidence = join(temporary, "evidence");
      const marker = join(temporary, "api-requests");
      await writeFile(script, source);
      const child = Bun.spawn([process.execPath, "--no-env-file", "--no-install", "--config=/dev/null", "--preload", fixture, script], {
        cwd: temporary,
        env: { GH_TOKEN: "acquisition-only", VELNOR_HOMEBREW_EVIDENCE_DIR: evidence, VELNOR_HOME_BUDGET_DEADLINE_EPOCH: String(Math.floor(Date.now() / 1000) + 60), FIXTURE_MODE: mode, API_MARKER: marker },
        stdout: "pipe", stderr: "pipe",
      });
      const stderr = await new Response(child.stderr).text();
      const status = await child.exited;
      const requests = await readFile(marker, "utf8");
      expect(requests).toContain("/repos/jackin-project/jackin");
      if (mode === "foreign-bundle") expect(requests).not.toContain("FOREIGN:");
      if (mode === "complete") {
        expect(stderr).toBe("");
        expect(status).toBe(0);
        expect((await readdir(evidence)).length).toBeGreaterThan(20);
        expect(requests).toContain("/attestations/sha256:");
      } else {
        expect(status).not.toBe(0);
        expect(stderr).toContain(mode === "changed-main" ? "changed during acquisition" : mode === "oversized" ? "response bound" : "foreign bundle URL");
      }
    } finally {
      await rm(temporary, { recursive: true, force: true });
    }
  });
}
