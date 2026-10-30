import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

// 子组件 emit 的事件如果父模板上没写监听，Vue 会静默丢弃：点了没反应、没有报错、
// 也没有日志。中转站卡片的「导出到其他应用」「CPA 续期」「重新授权」都这样坏过。
// 这条测试把整棵组件树扫一遍，任何一处漏接都会直接失败，不需要再手工点。
const ROOT = fileURLToPath(new URL("../src", import.meta.url));
const camel = (value: string) => value.replace(/-(\w)/g, (_, char: string) => char.toUpperCase());

/// 这些事件由组件自己的开关决定要不要渲染触发按钮，父级不监听是有意为之。
const GATED: Record<string, string> = {
  "components/ContentEditor.vue|cursor": "光标位置是可选回调，编辑器不监听也能用",
  "components/CodeEditor.vue|cursor": "同上",
  "components/agent-workspace/SessionReader.vue|close": "返回按钮只在 showBack 时渲染，会话记录弹窗里没有",
  "components/AgentNativeSettingField.vue|revealSource": "「原文」只在 configuration 模式下渲染，临时 CLI 面板是 launch 模式",
  "components/agent-workspace/AgentSessionList.vue|select": "勾选框只在 selectionMode 时渲染",
  "components/agent-workspace/AgentSessionList.vue|resume": "续跑入口由 allowResume 控制",
  "components/agent-workspace/AgentSessionList.vue|relocate": "迁移入口由 allowRelocation 控制",
  "components/agent-workspace/AgentSessionList.vue|activate": "激活入口由 allowResume 控制",
  "components/agent-workspace/AgentSessionList.vue|position": "滚动位置只在传了 positionKey 时才回传",
  "components/agent-workspace/AgentCatalogRow.vue|select": "勾选框只在 selectable 时渲染",
  "components/agent-workspace/AgentCatalogRow.vue|edit": "「编辑定义」只在 library 模式渲染，Hook 面板看的是状态栏资产",
  "components/agent-workspace/AgentCatalogPanel.vue|edit": "同上，面板只把行的编辑动作往上传",
  "components/agent-workspace/AgentCatalogConfigurationSelector.vue|compare": "同名资源比较入口由 can-compare 控制，批量弹窗里关掉了",
  "components/agent-workspace/AgentConfigurationEditorModal.vue|openSource": "文件切换按钮只在传了 sources 时渲染",
  "components/BatchOperationProgressModal.vue|showCheckInWindow": "按钮由 task.canShowWindow 控制，通用进度弹窗没有签到任务",
  "components/BatchOperationProgressModal.vue|resumeCheckIn": "同上",
  "components/BatchOperationProgressModal.vue|cancelCheckIn": "同上",
  "components/ProviderCard.vue|interaction": "卡片自己维护悬浮态，父级不需要接力",
};

/// 拖拽时叠出来的幽灵卡是纯装饰，不接任何交互事件。
const DECORATIVE = /draggedItem|draggedProvider|:interactive="false"|aria-hidden/;

interface ComponentInfo {
  emits: Set<string>;
  imports: Map<string, string>;
  usages: { tag: string; bound: Set<string>; bindsAll: boolean; snippet: string }[];
}

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (entry.endsWith(".vue")) out.push(path);
  }
  return out;
}

function describe(file: string): ComponentInfo {
  const source = readFileSync(file, "utf8");
  const imports = new Map<string, string>();
  for (const match of source.matchAll(/import\s+(\w+)\s+from\s+["']([^"']+\.vue)["']/g)) {
    imports.set(match[1], resolve(dirname(file), match[2]));
  }

  const emits = new Set<string>();
  const declared = source.match(/defineEmits<\{([\s\S]*?)\}>\(\)/);
  // 只取顶层成员：多行元组里的 `provider: Provider` 缩进更深，不是事件名。
  if (declared) {
    for (const match of declared[1].matchAll(/^ {2}([A-Za-z_$][\w$-]*)\??\s*:/gm)) {
      emits.add(camel(match[1]));
    }
  }
  for (const match of source.matchAll(/\bemit\(\s*["']([\w-]+)["']/g)) emits.add(camel(match[1]));

  const template = source.slice(source.indexOf("<template>"));
  const usages: ComponentInfo["usages"] = [];
  for (const match of template.matchAll(/<([A-Z][\w.]*)((?:[^>"']|"[^"]*"|'[^']*')*?)\/?>/g)) {
    const bound = new Set<string>();
    let bindsAll = false;
    for (const attr of match[2].matchAll(/(?:@|v-on:)([\w-]+(?::[\w-]+)?)|v-on="([^"]*)"/g)) {
      if (attr[1]) bound.add(camel(attr[1]));
      else bindsAll = true;
    }
    if (/\$attrs/.test(match[2])) bindsAll = true;
    usages.push({ tag: match[1], bound, bindsAll, snippet: match[0] });
  }
  return { emits, imports, usages };
}

test("every event a child emits is wired where the parent renders it", () => {
  const files = walk(ROOT);
  const components = new Map(files.map((file) => [file, describe(file)]));
  const offenders: string[] = [];

  for (const [file, parent] of components) {
    for (const usage of parent.usages) {
      if (usage.bindsAll || DECORATIVE.test(usage.snippet)) continue;
      const childFile = parent.imports.get(usage.tag);
      const child = childFile ? components.get(childFile) : undefined;
      if (!child) continue;
      for (const event of child.emits) {
        if (usage.bound.has(event)) continue;
        const key = `${relative(ROOT, childFile!)}|${event}`;
        if (key in GATED) continue;
        offenders.push(
          `${relative(ROOT, file)} 渲染 <${usage.tag}> 时没有监听 @${event}（${key}）`,
        );
      }
    }
  }

  assert.deepEqual(
    [...new Set(offenders)].sort(),
    [],
    "子组件事件在父模板上被静默丢弃：Vue 不会报错，用户只会看到按钮没反应",
  );
});
