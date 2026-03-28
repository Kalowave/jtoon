#!/usr/bin/env node
const { execFileSync } = require("child_process");
const path = require("path");
const os = require("os");

const binName = os.platform() === "win32" ? "jtoon.exe" : "jtoon";
const binPath = path.join(__dirname, "bin", binName);

try {
  execFileSync(binPath, process.argv.slice(2), { stdio: "inherit" });
} catch (e) {
  process.exit(e.status || 1);
}
