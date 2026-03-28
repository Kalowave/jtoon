const https = require("https");
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");
const os = require("os");

const VERSION = require("./package.json").version;

const TARGETS = {
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "win32-x64": "x86_64-pc-windows-msvc",
};

const platform = `${os.platform()}-${os.arch()}`;
const target = TARGETS[platform];

if (!target) {
  console.error(`jtoon: unsupported platform ${platform}`);
  process.exit(1);
}

const isWin = os.platform() === "win32";
const ext = isWin ? "zip" : "tar.gz";
const binName = isWin ? "jtoon.exe" : "jtoon";
const url = `https://github.com/Kalowave/jtoon/releases/download/v${VERSION}/jtoon-${target}.${ext}`;
const binDir = path.join(__dirname, "bin");
const binPath = path.join(binDir, binName);

if (fs.existsSync(binPath)) {
  process.exit(0);
}

fs.mkdirSync(binDir, { recursive: true });

const tmpFile = path.join(os.tmpdir(), `jtoon-${target}.${ext}`);

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const follow = (url) => {
      https
        .get(url, (res) => {
          if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
            follow(res.headers.location);
            return;
          }
          if (res.statusCode !== 200) {
            reject(new Error(`Download failed: HTTP ${res.statusCode}`));
            return;
          }
          const file = fs.createWriteStream(dest);
          res.pipe(file);
          file.on("finish", () => file.close(resolve));
        })
        .on("error", reject);
    };
    follow(url);
  });
}

async function main() {
  console.log(`Downloading jtoon v${VERSION} for ${platform}...`);
  await download(url, tmpFile);

  if (isWin) {
    execSync(`powershell -Command "Expand-Archive -Path '${tmpFile}' -DestinationPath '${binDir}'"`, { stdio: "ignore" });
  } else {
    execSync(`tar xzf "${tmpFile}" -C "${binDir}"`, { stdio: "ignore" });
  }

  fs.chmodSync(binPath, 0o755);
  fs.unlinkSync(tmpFile);
  console.log("jtoon installed successfully.");
}

main().catch((err) => {
  console.error(`jtoon install failed: ${err.message}`);
  process.exit(1);
});
