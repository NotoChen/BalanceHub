import fs from "node:fs";

const token = requiredEnv("GITHUB_TOKEN");
const repository = requiredEnv("GITHUB_REPOSITORY");
const releaseId = Number(requiredEnv("RELEASE_ID"));
const tag = requiredEnv("RELEASE_TAG");
const releaseBody = process.env.RELEASE_BODY ?? "";
const apiBase = (process.env.GITHUB_API_URL ?? "https://api.github.com").replace(/\/$/, "");

if (!Number.isSafeInteger(releaseId)) {
  throw new Error("RELEASE_ID 不是有效的 Release ID");
}

const apiHeaders = {
  Accept: "application/vnd.github+json",
  Authorization: `Bearer ${token}`,
  "User-Agent": "BalanceHub-release",
  "X-GitHub-Api-Version": "2022-11-28",
};

const packageJson = JSON.parse(fs.readFileSync("package.json", "utf8"));
const tauriConfig = JSON.parse(
  fs.readFileSync("src-tauri/tauri.conf.json", "utf8"),
);
const version = String(packageJson.version);
const productName = String(tauriConfig.productName);

if (tag.replace(/^v/i, "") !== version) {
  throw new Error(`Tag ${tag} 与 package.json 版本 ${version} 不一致`);
}

async function request(url, init = {}, attempts = 4) {
  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    let response;
    try {
      response = await fetch(url, {
        ...init,
        headers: { ...apiHeaders, ...(init.headers ?? {}) },
      });
    } catch (error) {
      lastError = error;
      if (attempt === attempts) throw error;
      await new Promise((resolve) => setTimeout(resolve, attempt * 2000));
      continue;
    }

    if (response.ok) return response;

    const body = await response.text();
    lastError = new Error(`${response.status} ${response.statusText}: ${body}`);
    if (response.status < 500 && response.status !== 429) throw lastError;
    if (attempt === attempts) throw lastError;
    await new Promise((resolve) => setTimeout(resolve, attempt * 2000));
  }

  throw lastError;
}

const release = await request(
  `${apiBase}/repos/${repository}/releases/${releaseId}`,
).then((response) => response.json());
const assets = await request(
  `${apiBase}/repos/${repository}/releases/${releaseId}/assets?per_page=100`,
).then((response) => response.json());
const assetsByName = new Map(assets.map((asset) => [asset.name, asset]));

function requiredAsset(name) {
  const asset = assetsByName.get(name);
  if (!asset) {
    throw new Error(`Release 缺少资产 ${name}`);
  }
  return asset;
}

function findAsset(pattern, label) {
  const matches = assets.filter((asset) => pattern.test(asset.name));
  if (matches.length !== 1) {
    throw new Error(`${label} 匹配到 ${matches.length} 个资产`);
  }
  return matches[0];
}

function escaped(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function downloadUrl(name) {
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;
}

async function readSignature(asset) {
  const signatureAsset = requiredAsset(`${asset.name}.sig`);
  const response = await request(signatureAsset.url, {
    headers: { Accept: "application/octet-stream" },
  });
  const signature = await response.text();
  if (!signature.trim()) {
    throw new Error(`资产 ${signatureAsset.name} 为空`);
  }
  return signature;
}

const product = escaped(productName);
const versionPattern = escaped(version);
const artifacts = {
  darwinAarch64: requiredAsset(`${productName}_aarch64.app.tar.gz`),
  darwinX64: requiredAsset(`${productName}_x64.app.tar.gz`),
  linuxAarch64AppImage: findAsset(
    new RegExp(`^${product}_${versionPattern}_aarch64\\.AppImage$`),
    "Linux ARM64 AppImage",
  ),
  linuxX64AppImage: findAsset(
    new RegExp(`^${product}_${versionPattern}_amd64\\.AppImage$`),
    "Linux x64 AppImage",
  ),
  linuxAarch64Deb: findAsset(
    new RegExp(`^${product}_${versionPattern}_arm64\\.deb$`),
    "Linux ARM64 deb",
  ),
  linuxX64Deb: findAsset(
    new RegExp(`^${product}_${versionPattern}_amd64\\.deb$`),
    "Linux x64 deb",
  ),
  linuxAarch64Rpm: findAsset(
    new RegExp(`^${product}-${versionPattern}-\\d+\\.aarch64\\.rpm$`),
    "Linux ARM64 rpm",
  ),
  linuxX64Rpm: findAsset(
    new RegExp(`^${product}-${versionPattern}-\\d+\\.x86_64\\.rpm$`),
    "Linux x64 rpm",
  ),
  windowsAarch64: findAsset(
    new RegExp(`^${product}_${versionPattern}_arm64-setup\\.exe$`),
    "Windows ARM64 安装包",
  ),
  windowsX64: findAsset(
    new RegExp(`^${product}_${versionPattern}_x64-setup\\.exe$`),
    "Windows x64 安装包",
  ),
};

const signatureEntries = await Promise.all(
  Object.entries(artifacts).map(async ([key, asset]) => [
    key,
    await readSignature(asset),
  ]),
);
const signatures = new Map(signatureEntries);
const platforms = {};

function addPlatform(key, asset, aliases = []) {
  const entry = {
    signature: signatures.get(assetKey(asset)),
    url: downloadUrl(asset.name),
  };
  platforms[key] = entry;
  for (const alias of aliases) {
    platforms[`${key}-${alias}`] = { ...entry };
  }
}

function assetKey(asset) {
  return Object.entries(artifacts).find(
    ([, candidate]) => candidate.name === asset.name,
  )?.[0];
}

function addLinuxPlatform(key, appImage, deb, rpm) {
  const variants = [
    [key, appImage],
    [`${key}-appimage`, appImage],
    [`${key}-deb`, deb],
    [`${key}-rpm`, rpm],
  ];
  for (const [platformKey, asset] of variants) {
    platforms[platformKey] = {
      signature: signatures.get(assetKey(asset)),
      url: downloadUrl(asset.name),
    };
  }
}

addLinuxPlatform(
  "linux-aarch64",
  artifacts.linuxAarch64AppImage,
  artifacts.linuxAarch64Deb,
  artifacts.linuxAarch64Rpm,
);
addLinuxPlatform(
  "linux-x86_64",
  artifacts.linuxX64AppImage,
  artifacts.linuxX64Deb,
  artifacts.linuxX64Rpm,
);
addPlatform("darwin-aarch64", artifacts.darwinAarch64, ["app"]);
addPlatform("darwin-x86_64", artifacts.darwinX64, ["app"]);
addPlatform("windows-aarch64", artifacts.windowsAarch64, ["nsis"]);
addPlatform("windows-x86_64", artifacts.windowsX64, ["nsis"]);

const content = `${JSON.stringify(
  {
    version,
    notes: releaseBody,
    pub_date: new Date().toISOString(),
    platforms,
  },
  null,
  2,
)}\n`;
fs.writeFileSync("latest.json", content);

if (process.env.DRY_RUN === "true") {
  console.log(`已生成 latest.json（${Object.keys(platforms).length} 个平台入口，未上传）`);
  process.exit(0);
}

const existingManifest = assetsByName.get("latest.json");
if (existingManifest) {
  await request(
    `${apiBase}/repos/${repository}/releases/assets/${existingManifest.id}`,
    { method: "DELETE" },
  );
}

const uploadUrl = release.upload_url.replace(/\{\?.*\}$/, "");
await request(`${uploadUrl}?name=latest.json`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: content,
});

console.log(`已生成并上传 latest.json（${Object.keys(platforms).length} 个平台入口）`);

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`缺少环境变量 ${name}`);
  return value;
}
