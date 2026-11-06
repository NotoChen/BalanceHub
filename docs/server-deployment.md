---
layout: default
title: 服务端部署
description: 把 BalanceHub 的网络能力部署为无界面后端，用浏览器访问同一套界面。
page_class: document-page
---

# 服务端部署

BalanceHub 有两种运行形态：

- **桌面版**：安装包启动，带 Tauri 窗口，管理本机 Agent CLI、终端和浏览器登录。
- **无界面后端**：`balancehub-server` 常驻运行，托管前端页面，用浏览器访问。额度刷新、签到、云端同步、OAuth / CPA 管理、站点浏览器登录以及 Agent 工作台都在服务端执行，后者只需要服务器上装了对应 CLI。

两种形态共用 `balancehub-core` 与 `balancehub-agent` 的实现和同一份配置格式；差别只在需要桌面窗口的入口，由运行环境决定是否展示。

## 从发布包运行

Release 里的 `balancehub-server-linux-x64.tar.gz` / `arm64.tar.gz` 已经包含可执行文件和 `web/` 前端：

```sh
mkdir -p ~/balancehub-server
tar -xzf balancehub-server-linux-x64.tar.gz -C ~/balancehub-server --strip-components=1
cd ~/balancehub-server
BALANCEHUB_ROOT="$HOME/.balancehub" ./balancehub-server
```

服务端会在可执行文件旁查找 `web/`，因此不需要设置 `BALANCEHUB_WEB_DIR`。监听地址默认 `127.0.0.1:8787`；需要局域网访问时设置 `BALANCEHUB_SERVER_ADDR=0.0.0.0:8787`。

长期运行可以用 systemd（先创建专用系统用户，例如 `useradd --system --home /var/lib/balancehub balancehub`）：

```ini
[Unit]
Description=BalanceHub Server
After=network-online.target
Wants=network-online.target

[Service]
User=balancehub
Group=balancehub
WorkingDirectory=/opt/balancehub
ExecStart=/opt/balancehub/balancehub-server
Environment=BALANCEHUB_ROOT=/var/lib/balancehub
Environment=BALANCEHUB_SERVER_ADDR=127.0.0.1:8787
StateDirectory=balancehub
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

把压缩包解压到 `/opt/balancehub`，`StateDirectory` 会创建并授权 `/var/lib/balancehub`。升级时替换可执行文件与 `web/` 后重启服务。

## 从源码运行

先构建前端，再构建服务端：

```sh
npm ci
npm run build
npm run cargo -- build --release -p balancehub-server
```

产物位置由 `scripts/build-cache.mjs` 的 `CARGO_TARGET_DIR` 决定（macOS 默认是 `~/Library/Caches/BalanceHub/cargo-target`），可执行文件在它的 `release/` 目录下。运行时把前端目录指过去：

```sh
BALANCEHUB_ROOT="$HOME/.balancehub" \
BALANCEHUB_WEB_DIR="$PWD/dist" \
BALANCEHUB_SERVER_ADDR=127.0.0.1:8787 \
  /path/to/balancehub-server
```

启动后访问 `http://127.0.0.1:8787`。健康检查是 `GET /api/health`，返回 `{"ok":true,"version":"…"}`；`GET /api/status` 另外返回运行时长、数据修订号、事件流连接数和最近一次后台刷新/签到/测活任务的状态与错误，适合接监控或排查「为什么没有自动刷新」。

## 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `BALANCEHUB_ROOT` | `~/.balancehub` | 数据目录根。其余目录默认都落在它下面。 |
| `BALANCEHUB_DATA_DIR` | `$BALANCEHUB_ROOT` | 配置文件、备份与浏览器组件目录。 |
| `BALANCEHUB_CONFIG_DIR` | `$BALANCEHUB_ROOT/config` | 云端同步状态等宿主配置。 |
| `BALANCEHUB_CACHE_DIR` | `$BALANCEHUB_ROOT/cache` | 临时缓存。 |
| `BALANCEHUB_SERVER_ADDR` | `127.0.0.1:8787` | 监听地址。容器或局域网访问时改为 `0.0.0.0:8787`。 |
| `BALANCEHUB_ALLOWED_HOSTS` | 回环地址、监听地址与 IP 直连 | 逗号分隔的额外 Host 白名单。反向代理改用其他域名时加入该域名；`*` 表示不校验 Host。 |
| `BALANCEHUB_ALLOWED_ORIGINS` | 同源 | 逗号分隔的额外 Origin 白名单，例如 `https://balancehub.example.com`。 |
| `BALANCEHUB_WEB_DIR` | 可执行文件旁的 `web/`，否则仓库 `dist/` | 前端静态文件目录。 |
| `BALANCEHUB_LOG` | `info` | 日志级别：`error` / `warn` / `info` / `debug` / `trace`，输出到标准输出。 |
| `TZ` | 系统默认 | 自动签到按本地时间判定执行点。 |

数据目录里主要是：

- `data.json`：中转站、OAuth 账号、CPA 网关与设置。
- `data.json.v{N}.bak`：配置迁移前的原文件备份。
- `data.lock`：配置目录的进程独占锁；运行期间一直持有，进程退出后自动释放，不需要手动删除。
- `oauth-device-id`：本机（本服务端）的续期设备标识，不参与云端同步。
- `components/checkin-browser/`：签到用的浏览器组件，首次安装时才会下载。

**同一份数据目录只能由一个进程使用。** 启动时会为配置目录加操作系统文件锁；桌面版与服务端同时指向同一目录时，后启动的一方会直接报错退出，避免双方各持一份快照互相覆盖。进程崩溃后锁会自动释放，可以直接重新启动。

**服务端没有内置登录。** `/rpc` 只接受 `application/json`，跨站 `text/plain` 简单请求会收到 415；`Host` / `Origin` 不匹配的请求收到 403，包括 DNS rebinding 和 `Origin: null`。单次批处理最多 64 条命令。这些护栏挡住的是浏览器跨站请求，不能替代鉴权：把端口暴露给不受信任网络前，请在反向代理上加认证，并通过 `BALANCEHUB_ALLOWED_HOSTS` / `BALANCEHUB_ALLOWED_ORIGINS` 声明外部域名。监听非回环地址时启动日志会输出同样的提醒。

## 容器部署

仓库根目录提供 `Dockerfile` 和 `compose.yaml`。镜像分三段构建：前端 `dist/`、只编译 `balancehub-server` 的 Rust 构建、带 Chromium 运行库的 Debian 运行层。构建使用 BuildKit 的缓存挂载，需要 Docker 23 及以上。

```sh
docker compose up -d --build
```

等价的手动构建与运行：

```sh
docker build -t balancehub-server .
docker run -d --name balancehub \
  -p 127.0.0.1:8787:8787 \
  -v balancehub-data:/data \
  -e TZ=Asia/Shanghai \
  balancehub-server
```

要点：

- 镜像内监听 `0.0.0.0:8787`，宿主侧是否对外开放由端口映射决定。默认示例只映射到 `127.0.0.1`。
- `docker stop`、systemd `stop` 和 Ctrl+C 都会发送退出信号；服务端停止接受新连接，等在途请求结束后再退出。
- 所有状态都在 `/data` 卷里；重建容器不会丢配置。
- 需要签到和浏览器组件时不要裁掉运行层里的 Chromium 依赖；组件首次安装会下载自己的 Node、Playwright 与 Chromium 到 `/data/components/`。
- 容器默认以 root 运行；改用其他用户时，先确保 `/data` 对该用户可写。
- 仓库不发布预构建镜像，镜像由你本地构建，或自行推到自己的镜像仓库。

## 反向代理

服务端是同源应用：页面、`POST /rpc` 与 `GET /events`（WebSocket）在同一个端口。反代要同时转发这三条路径，并让 `/events` 允许升级、关闭缓冲。

反代改了外部域名后，服务端看到的 `Host` / `Origin` 不再是回环地址。启动服务端时要显式声明这个域名，否则请求会被请求护栏拒绝并返回 403：

```sh
BALANCEHUB_ALLOWED_HOSTS=balancehub.example.com \
BALANCEHUB_ALLOWED_ORIGINS=https://balancehub.example.com \
BALANCEHUB_SERVER_ADDR=127.0.0.1:8787 \
  /path/to/balancehub-server
```

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    ''      close;
}

server {
    listen 443 ssl;
    server_name balancehub.example.com;

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_read_timeout 3600s;
        proxy_buffering off;
    }
}
```

事件流断开后前端会自动重连；刷新页面也会重新读取当前状态，不需要额外配置。每个标签页各自建立一条事件流连接并携带客户端标识：资产目录与资源正文的进度只发给发起请求的那个标签页；标签页关闭一分钟后释放它持有的资产访问身份与读取配额。

## 签到与浏览器组件

服务端复用同一套浏览器组件。签到始终以**无头模式**运行；站点浏览器登录需要可交互的浏览器窗口，是否可用由运行环境决定：

- 在 **设置 → 登录与授权 → 浏览器组件** 中安装或选择组件。首次安装会下载 Node、Playwright 与 Chromium，耗时和磁盘占用都明显高于桌面版。
- 无头环境不提供「显示登录窗口」，登录入口也会隐藏；需要人工过盾的站点只能依赖站点自身的免验证策略。
- 组件安装在数据目录下，随 `/data` 卷保留。

## OAuth 与 CPA

- 授权任务在服务端创建和执行；授权页面在你自己的浏览器里打开。
- 服务端的本机回调地址通常到不了你的浏览器：浏览器停在 `localhost` 回调地址后，把地址栏里的完整链接粘贴回面板即可完成保存。
- 导入授权文件读取上传的文件文本；导出直接下载 JSON。服务端不接收客户端文件路径，也不会把授权写到服务器磁盘。
- 账号详情里的「本机自动续期」指服务端以自身的设备标识续期；同一账号在桌面与服务器之间交接时，注意只让一边持有续期。

## 能力边界

服务端提供：中转站与 API Key 管理、额度与用量、连接测试、签到、云端同步、OAuth 账号、CPA 网关、配置导入导出、站点公告，以及 **Agent 工作台**——CLI 发现、配置读写、会话检索、共享资产、MCP / Hook 管理与自动升级，只要服务器上装了对应 CLI 就能用。

按运行环境自动判断：

- **终端启动、系统文件打开**：需要可交互的桌面会话。Linux 上没有 `DISPLAY` / `WAYLAND_DISPLAY` 时，卡片上的「启动临时 CLI」和配置来源的「系统打开」不可用，界面直接隐藏入口。
- **站点浏览器登录**：打开登录窗口需要可交互的桌面会话。本地分离部署或带显示环境的服务器可以像桌面版一样登录并导入账号；无显示环境下界面隐藏「登录 / 打开账号」入口，登录账号管理、Cookie 查看和浏览器组件安装仍然可用，签到以无头模式运行。
- **应用自动更新**：只由桌面安装包提供，服务端没有这些命令，界面隐藏。

测活与自动升级执行在服务器本机：服务器上装了 Agent CLI 就会真实运行并产生额度消耗。

## 安全与备份

- 服务端没有内置登录与鉴权，按单人自用设计。请绑定回环地址，或放在带认证的反向代理之后，不要直接暴露到公网。
- 配置里包含账号凭据与授权令牌，`data.json` 与备份应按秘密数据保护，卷不要共享给其他服务。
- 升级前先备份：桌面与服务端都支持导出完整配置；也可以使用云端同步保留一份云端副本。
- 替换可执行文件或镜像后重启即可；配置文件由 schema 迁移自动升级，原文件会留在同目录的 `data.json.v{N}.bak`。

## 排错

- 页面打不开：确认 `BALANCEHUB_WEB_DIR` 指向包含 `index.html` 的目录，或可执行文件旁存在 `web/`。
- 页面能开但操作超时：检查反向代理是否转发了 `/rpc`，以及 `/events` 是否允许 WebSocket 升级。
- 首屏一直显示加载：查看服务端标准输出（`BALANCEHUB_LOG=debug` 可提高详细程度）。
- 签到一直失败：先在浏览器组件里完成安装与验证，并确认数据目录可写、容器内 Chromium 依赖未被裁掉。
