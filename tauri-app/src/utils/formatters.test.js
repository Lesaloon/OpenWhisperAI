import test from "node:test";
import assert from "node:assert/strict";
import { formatBytes, formatDuration, formatPercent, formatRate } from "./formatters.js";

test("formatBytes returns human readable units", () => {
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(1024), "1.0 KB");
  assert.equal(formatBytes(1048576), "1.0 MB");
});

test("formatDuration returns minutes and seconds", () => {
  assert.equal(formatDuration(0), "0s");
  assert.equal(formatDuration(45), "45s");
  assert.equal(formatDuration(125), "2m 05s");
});

test("formatPercent clamps values", () => {
  assert.equal(formatPercent(-4), "0%");
  assert.equal(formatPercent(32), "32%");
  assert.equal(formatPercent(140), "100%");
});

test("formatRate formats throughput", () => {
  assert.equal(formatRate(0), "-");
  assert.equal(formatRate(1048576), "1.0 MB/s");
});
