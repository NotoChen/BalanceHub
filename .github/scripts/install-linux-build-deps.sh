#!/usr/bin/env bash
set -euo pipefail

readonly bh_mirror_file="/etc/apt/apt-mirrors.txt"
readonly bh_primary_mirror="${BALANCEHUB_APT_MIRROR:-https://archive.ubuntu.com/ubuntu/}"
readonly -a bh_packages=(
  libwebkit2gtk-4.1-dev
  libayatana-appindicator3-dev
  librsvg2-dev
  rpm
  xdg-utils
  patchelf
)

# GitHub 的 Ubuntu x64 runner 偶尔会把 azure.archive.ubuntu.com 留在
# mirror+file 的首选位置；该镜像不可达时 apt 会长时间挂起，直到整个 job
# 超时。固定为 Ubuntu 官方 HTTPS 镜像，并让每次尝试都有独立时限。
if [[ -f "$bh_mirror_file" ]]; then
  printf '%s\n' "$bh_primary_mirror" | sudo tee "$bh_mirror_file" >/dev/null
fi

for bh_attempt in 1 2; do
  if sudo env DEBIAN_FRONTEND=noninteractive \
    timeout --kill-after=15s 120s \
    apt-get \
      -o Acquire::Retries=3 \
      -o Acquire::http::Timeout=20 \
      -o Acquire::https::Timeout=20 \
      -o Dpkg::Use-Pty=0 \
      update \
    && sudo env DEBIAN_FRONTEND=noninteractive \
      timeout --kill-after=15s 240s \
      apt-get \
        -o Acquire::Retries=3 \
        -o Acquire::http::Timeout=20 \
        -o Acquire::https::Timeout=20 \
        -o Dpkg::Use-Pty=0 \
        install -y "${bh_packages[@]}"
  then
    exit 0
  fi

  if [[ "$bh_attempt" -lt 2 ]]; then
    echo "Linux 构建依赖安装失败，5 秒后进行最后一次重试"
    sleep 5
  fi
done

echo "Linux 构建依赖安装失败"
exit 1
