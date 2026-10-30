#!/usr/bin/env bash
# 开发版启动脚本：安装版配置的只读副本 + 完整演示卡片，落在隔离目录。
# 用法：scripts/dev-session.sh        （仓库根目录执行，或 npm run dev:session）
# 变量统一 BALANCEHUB_ / bh_ 前缀。
set -e

bh_repo_root="$(cd -- "$(dirname -- "$0")/.." && pwd)"
bh_installed_config="$HOME/Library/Application Support/com.zangbai.balancehub"
bh_dev_root="${BALANCEHUB_DEV_ROOT:-$bh_repo_root/target/dev-session}"
bh_copy_root="${BALANCEHUB_DEV_COPY_ROOT:-$HOME/Library/Caches/BalanceHub/dev-session}"

if pgrep -f "[c]argo-target/debug/balancehub" >/dev/null 2>&1; then
  echo >&2 "已有开发版进程在运行，先退出它再执行本脚本。"
  exit 1
fi

echo "== 1/3 复制安装版配置（只读副本，不修改原目录）"
rm -rf -- "$bh_copy_root"
mkdir -p -- "$bh_copy_root"
if [[ -d "$bh_installed_config" ]]; then
  ditto "$bh_installed_config" "$bh_copy_root" || exit 1
  echo "   已复制 $(find "$bh_copy_root" -type f | wc -l | tr -d ' ') 个文件 → $bh_copy_root"
else
  echo "   未找到安装版配置目录，跳过复制"
fi

echo "== 2/3 生成隔离开发根目录：$bh_dev_root"
rm -rf -- "$bh_dev_root"
mkdir -p -- "$bh_dev_root/config" "$bh_dev_root/sample" "$bh_dev_root/data"
# 配置副本只取 data.json，其余运行状态（缓存、日志）让开发版自己生成。
if [[ -f "$bh_copy_root/data.json" ]]; then
  cp -- "$bh_copy_root/data.json" "$bh_dev_root/config/data.json"
  echo "   配置副本 data.json 已就位（$(wc -c < "$bh_dev_root/config/data.json" | tr -d ' ') 字节）"
fi

echo "== 3/3 启动 tauri dev（BALANCEHUB_DEV_ROOT=$bh_dev_root，并入演示卡片）"
cd -- "$bh_repo_root" || exit 1
export BALANCEHUB_DEV_ROOT="$bh_dev_root"
# 开发版首次加载配置时并入「演示 ·」卡片，之后的启动直接读已写好的配置。
export BALANCEHUB_DEV_SAMPLE=1
exec npm run tauri dev
