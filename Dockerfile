# 浏览器版前端：构建 dist/，由服务端直接托管。
FROM node:22-bookworm-slim AS web
WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci
COPY index.html tsconfig.json tsconfig.node.json vite.config.ts ./
COPY src-tauri/tauri.conf.json ./src-tauri/tauri.conf.json
COPY src ./src
RUN npm run build

# 无界面后端：只编译 balancehub-server，不引入 Tauri 依赖。
FROM rust:1-bookworm AS server
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY browser-worker ./browser-worker
COPY crates ./crates
COPY src-tauri ./src-tauri
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked -p balancehub-server \
    && mkdir -p /out \
    && cp /build/target/release/balancehub-server /out/

FROM debian:bookworm-slim AS runtime
# 浏览器组件的系统依赖：无头签到时组件会下载自己的 Chromium 与 Node。
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        tzdata \
        libstdc++6 \
        libnss3 libnspr4 libdbus-1-3 \
        libatk1.0-0 libatk-bridge2.0-0 libatspi2.0-0 \
        libcups2 libdrm2 libgbm1 \
        libx11-xcb1 libxcomposite1 libxdamage1 libxfixes3 libxkbcommon0 libxrandr2 libxshmfence1 \
        libpango-1.0-0 libcairo2 libasound2 \
        fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=server /out/balancehub-server /usr/local/bin/balancehub-server
COPY --from=web /build/dist ./web

# 容器里没有用户主目录可写，数据目录显式落在卷上；监听地址必须对容器外可见。
ENV BALANCEHUB_ROOT=/data \
    BALANCEHUB_WEB_DIR=/app/web \
    BALANCEHUB_SERVER_ADDR=0.0.0.0:8787

VOLUME ["/data"]
EXPOSE 8787
ENTRYPOINT ["balancehub-server"]
