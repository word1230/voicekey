// Rust 的 Settings（serde camelCase）与 TS 的 Settings 接口靠手写同步。
// 漏一个字段 tsc 不会报（接口多/少字段只在读写时暴露），合并上游时最容易两边各加各的。
// 这里把两份字段名对一遍，差异直接让 npm test 红。
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const camel = (s) => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());

function rustSettingsFields() {
  const src = readFileSync(new URL("../../src-tauri/src/settings.rs", import.meta.url), "utf8");
  const start = src.indexOf("pub struct Settings {");
  assert.notEqual(start, -1, "settings.rs 里找不到 pub struct Settings");
  const body = src.slice(start, src.indexOf("\n}", start));
  return [...body.matchAll(/^\s*pub\s+(\w+)\s*:/gm)].map((m) => camel(m[1]));
}

function tsSettingsFields() {
  const src = readFileSync(new URL("./api.ts", import.meta.url), "utf8");
  const start = src.indexOf("export interface Settings {");
  assert.notEqual(start, -1, "api.ts 里找不到 export interface Settings");
  const body = src.slice(start, src.indexOf("\n}", start)).replace(/\/\*[\s\S]*?\*\//g, "");
  return [...body.matchAll(/([A-Za-z][A-Za-z0-9_]*)\??\s*:/g)].map((m) => m[1]);
}

test("Rust Settings 与 TS Settings 的字段一一对应", () => {
  const rust = new Set(rustSettingsFields());
  const ts = new Set(tsSettingsFields());
  const missingInTs = [...rust].filter((f) => !ts.has(f)).sort();
  const extraInTs = [...ts].filter((f) => !rust.has(f)).sort();
  assert.deepEqual(
    { missingInTs, extraInTs },
    { missingInTs: [], extraInTs: [] },
    "加/删 Settings 字段时，app/src-tauri/src/settings.rs 与 app/src/lib/api.ts 要一起改",
  );
});
