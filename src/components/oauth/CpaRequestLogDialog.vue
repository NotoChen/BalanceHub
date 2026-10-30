<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { Pause, Play, RefreshCw, Trash2 } from "@lucide/vue";
import type { Provider } from "../../stores/provider-types";
import type {
  CpaLogSettings,
  CpaLogTail,
  CpaRequestLog,
} from "../../stores/oauth-types";
import {
  clearCpaLogs,
  getCpaLogSettings,
  readCpaRequestLog,
  readCpaRequestLogs,
  setCpaLogSettings,
} from "../../api/oauth-accounts";
import { accountTimestamp } from "../../utils/oauth-display";
import { cpaLogLevels, parseCpaLogLine } from "../../utils/cpa-log";
import { confirmAction } from "../../composables/provider-credential-dialogs";
const props = defineProps<{ visible: boolean; provider: Provider }>();
const emit = defineEmits<{ close: [] }>();
const loading = ref(false);
const tail = ref<CpaLogTail | null>(null);
const error = ref("");
const notice = ref("");
const settings = ref<CpaLogSettings | null>(null);
const switching = ref(false);
const clearing = ref(false);
const following = ref(false);
/// The separate file the gateway keeps for one request, opened from the request
/// ID on a line. It is a drill-down over the tail, not a second log source.
const requestLog = ref<CpaRequestLog | null>(null);
const readingRequest = ref(false);
/// The filters are a view over the buffer: following keeps appending, so a line
/// that matches while the dialog is open appears without another gateway read.
const filter = ref("");
/// `all` is an explicit choice, so clearing a selection is the same state as
/// never having filtered and the count below stays truthful.
const levelFilter = ref("all");
const parsed = computed(() =>
  (tail.value?.lines || []).map(parseCpaLogLine),
);
const levels = computed(() =>
  cpaLogLevels(parsed.value.map((line) => line.level)),
);
const shown = computed(() => {
  const query = filter.value.trim().toLowerCase();
  return parsed.value.filter(
    (line) =>
      (levelFilter.value === "all" || line.level === levelFilter.value) &&
      (!query || line.raw.toLowerCase().includes(query)),
  );
});
let timer: ReturnType<typeof setInterval> | undefined;
/// The gateway caps a read at 500 lines; the buffer keeps a few reads' worth so
/// following for a while does not grow without bound.
const LOG_LIMIT = 200;
const LOG_MAX_LINES = 1000;
const LOG_POLL_MS = 5_000;
/// Reads share one sequence: a close, a manual refresh or a provider switch
/// makes the reads in flight stale, while a follow poll only continues the
/// state it started from.
let sequence = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  stopFollow();
  sequence++;
});
watch(
  () => props.visible,
  (visible) => {
    if (visible) void load();
    else reset();
  },
  { immediate: true },
);
watch(
  () => props.provider.identity.id,
  () => {
    if (props.visible) void load();
  },
);
/// Reads a tail and publishes it when it still belongs to the current sequence.
async function readTail(current: number): Promise<CpaLogTail | null> {
  try {
    const result = await readCpaRequestLogs(
      props.provider.identity.id,
      LOG_LIMIT,
    );
    if (disposed || current !== sequence) return null;
    tail.value = result;
    return result;
  } catch (cause) {
    if (!disposed && current === sequence) error.value = String(cause);
    return null;
  }
}
/// The log is only served while the gateway writes it to disk, so the switch is
/// read first: an empty log is then explained by the setting that causes it
/// instead of by an error the user cannot act on.
async function load() {
  stopFollow();
  tail.value = null;
  requestLog.value = null;
  filter.value = "";
  levelFilter.value = "all";
  error.value = "";
  notice.value = "";
  settings.value = null;
  loading.value = true;
  const current = ++sequence;
  try {
    const read = await getCpaLogSettings(props.provider.identity.id);
    if (disposed || current !== sequence) return;
    settings.value = read;
    if (read.enabled) await readTail(current);
  } catch (cause) {
    if (!disposed && current === sequence) error.value = String(cause);
  } finally {
    if (current === sequence) loading.value = false;
  }
}
/// A manual refresh restarts from the newest tail; a follow that keeps running
/// picks up the cursor this read returns.
async function refresh() {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  notice.value = "";
  requestLog.value = null;
  const current = ++sequence;
  await readTail(current);
  if (current === sequence) loading.value = false;
}
/// One follow step: ask for what was written after the last line. The gateway
/// says when it rotated the file and could not resolve the cursor; the buffer
/// then becomes the fresh tail instead of growing with unrelated lines.
async function poll() {
  const current = tail.value;
  if (!current?.nextCursor || loading.value) return;
  const reading = sequence;
  try {
    const result = await readCpaRequestLogs(
      props.provider.identity.id,
      LOG_LIMIT,
      current.nextCursor,
    );
    if (disposed || reading !== sequence) return;
    tail.value = {
      ...result,
      lines: result.cursorReset
        ? result.lines
        : [...current.lines, ...result.lines].slice(-LOG_MAX_LINES),
    };
  } catch (cause) {
    // A follow read that fails stops following instead of retrying a gateway
    // that is refusing every call; the reason stays next to the log.
    if (!disposed && reading === sequence) {
      error.value = String(cause);
      stopFollow();
    }
  }
}
function stopFollow() {
  if (timer) clearInterval(timer);
  timer = undefined;
  following.value = false;
}
function toggleFollow() {
  if (following.value) {
    stopFollow();
    return;
  }
  if (!tail.value?.nextCursor) return;
  following.value = true;
  timer = setInterval(() => void poll(), LOG_POLL_MS);
}
/// Opens the file the gateway keeps for this one request. The line only carries
/// the shortened ID, which is exactly the suffix the gateway matches, so it is
/// passed through unchanged.
async function openRequest(requestId: string) {
  if (readingRequest.value) return;
  error.value = "";
  readingRequest.value = true;
  const current = sequence;
  try {
    const result = await readCpaRequestLog(
      props.provider.identity.id,
      requestId,
    );
    if (!disposed && current === sequence) requestLog.value = result;
  } catch (cause) {
    if (!disposed && current === sequence) error.value = String(cause);
  } finally {
    if (current === sequence) readingRequest.value = false;
  }
}
async function toggleLogFile(enabled: boolean) {
  if (switching.value) return;
  const id = props.provider.identity.id;
  const current = props.provider.auth.credentialRevision || 0;
  const confirmed = await confirmAction(
    enabled ? "开启 CPA 日志写入文件" : "关闭 CPA 日志写入文件",
    enabled
      ? "CPA 会持续把日志写入网关主机的日志目录，网关会重新加载配置。开启后才能在这里查看请求日志。"
      : "关闭后 CPA 不再写入日志文件，请求日志立即不可用；已经写下的日志文件不会被删除。",
    enabled ? "开启日志写入" : "关闭日志写入",
    "warning",
  );
  if (!confirmed || disposed || props.provider.identity.id !== id) return;
  switching.value = true;
  error.value = "";
  notice.value = "";
  const reading = ++sequence;
  try {
    await setCpaLogSettings(id, enabled, current);
    if (disposed || reading !== sequence) return;
    settings.value = { enabled };
    if (!enabled) {
      stopFollow();
      tail.value = null;
      requestLog.value = null;
      return;
    }
    await readTail(reading);
  } catch (cause) {
    if (!disposed && reading === sequence) error.value = String(cause);
  } finally {
    if (reading === sequence) switching.value = false;
  }
}
/// Clearing is destructive on the gateway host, so it says exactly what goes
/// away: the file being written is emptied, and the rotated ones are deleted.
async function clearLogs() {
  if (clearing.value) return;
  const id = props.provider.identity.id;
  const current = props.provider.auth.credentialRevision || 0;
  const confirmed = await confirmAction(
    "清理 CPA 请求日志",
    "网关会清空当前正在写入的日志文件，并删除已经轮转的日志文件；这些日志无法恢复。清理后网关会继续写入新的日志。",
    "清理日志",
    "warning",
  );
  if (!confirmed || disposed || props.provider.identity.id !== id) return;
  clearing.value = true;
  error.value = "";
  notice.value = "";
  const reading = ++sequence;
  try {
    const result = await clearCpaLogs(id, current);
    if (disposed || reading !== sequence) return;
    stopFollow();
    tail.value = null;
    requestLog.value = null;
    filter.value = "";
    levelFilter.value = "all";
    notice.value = result.removed
      ? `已清空当前日志文件，并删除 ${result.removed} 个已轮转的日志文件。`
      : "已清空当前日志文件。";
    await readTail(reading);
  } catch (cause) {
    if (!disposed && reading === sequence) error.value = String(cause);
  } finally {
    if (reading === sequence) clearing.value = false;
  }
}
function reset() {
  stopFollow();
  sequence++;
  tail.value = null;
  requestLog.value = null;
  filter.value = "";
  levelFilter.value = "all";
  error.value = "";
  notice.value = "";
  settings.value = null;
  // A read or a write that was in flight belongs to the closed dialog; its own
  // sequence can no longer clear these, so the close does it instead of leaving
  // the panel permanently busy.
  loading.value = false;
  switching.value = false;
  clearing.value = false;
  readingRequest.value = false;
}
</script>

<template>
  <a-modal
    :visible="visible"
    :title="
      requestLog ? `CPA 请求日志 · ${requestLog.requestId}` : 'CPA 请求日志'
    "
    width="880px"
    :footer="false"
    unmount-on-close
    @cancel="emit('close')"
  >
    <a-alert v-if="error" type="warning">{{ error }}</a-alert>
    <a-alert v-if="notice" type="success" :show-icon="false">{{
      notice
    }}</a-alert>
    <a-spin
      v-if="loading || readingRequest || clearing"
      tip="读取 CPA 请求日志"
    />
    <template v-else-if="requestLog">
      <a-button size="mini" class="cpa-log-back" @click="requestLog = null"
        >返回日志</a-button
      >
      <pre class="cpa-log-content">{{
        requestLog.content || "这次请求的日志文件是空的。"
      }}</pre>
    </template>
    <template v-else-if="settings && !settings.enabled">
      <p class="cpa-empty">
        CPA 当前没有把日志写入文件，请求日志接口因此不会返回内容。开启后网关会重新加载配置，之后写入的日志会出现在这里。
      </p>
      <div class="cpa-log-toolbar cpa-log-toolbar-end">
        <a-button
          size="mini"
          type="primary"
          :loading="switching"
          @click="toggleLogFile(true)"
          >开启日志写入</a-button
        >
      </div>
    </template>
    <template v-else-if="tail">
      <a-alert v-if="tail.cursorReset" type="info" :show-icon="false">
        网关已轮转日志文件，下面是从当前末尾重新读取的内容。
      </a-alert>
      <div class="cpa-log-toolbar">
        <span
          >{{ shown.length
          }}<template v-if="filter.trim() || levelFilter !== 'all'"
            > / {{ tail.lines.length }}</template
          >
          行<template v-if="tail.latestAt">
            · 最新 {{ accountTimestamp(tail.latestAt) }}</template
          ></span
        >
        <a-space :size="6">
          <a-button
            size="mini"
            :type="following ? 'primary' : 'secondary'"
            :disabled="!tail.nextCursor"
            @click="toggleFollow"
            ><template #icon>
              <Pause v-if="following" :size="12" />
              <Play v-else :size="12" /> </template
            >{{ following ? "停止跟随" : "跟随" }}</a-button
          >
          <a-button size="mini" :loading="loading" @click="refresh"
            ><template #icon><RefreshCw :size="12" /></template>刷新</a-button
          >
          <a-button
            size="mini"
            status="warning"
            :loading="switching"
            @click="toggleLogFile(false)"
            >关闭日志写入</a-button
          >
          <a-button
            size="mini"
            status="danger"
            :loading="clearing"
            @click="clearLogs"
            ><template #icon><Trash2 :size="12" /></template>清理日志</a-button
          >
        </a-space>
      </div>
      <div class="cpa-log-filters">
        <a-input
          v-model="filter"
          class="cpa-log-filter"
          size="mini"
          allow-clear
          placeholder="筛选日志行，按包含匹配"
        />
        <a-select
          v-model="levelFilter"
          class="cpa-log-level-filter"
          size="mini"
        >
          <a-option value="all">全部级别</a-option>
          <a-option v-for="level in levels" :key="level" :value="level">{{
            level
          }}</a-option>
        </a-select>
      </div>
      <div v-if="shown.length" class="cpa-log-lines">
        <div v-for="(line, index) in shown" :key="index" class="cpa-log-line">
          <span class="cpa-log-time">{{ line.timestamp }}</span>
          <button
            v-if="line.requestId"
            type="button"
            class="cpa-log-request"
            :title="`查看请求 ${line.requestId} 的独立日志`"
            @click="openRequest(line.requestId)"
          >
            {{ line.requestId }}
          </button>
          <span v-else class="cpa-log-request cpa-log-request-none">·</span>
          <span v-if="line.level" class="cpa-log-level" :class="`is-${line.level}`">{{
            line.level
          }}</span>
          <span v-if="line.source" class="cpa-log-source">{{
            line.source
          }}</span>
          <span class="cpa-log-message">{{ line.message }}</span>
        </div>
      </div>
      <p v-else-if="tail.lines.length" class="cpa-empty">
        没有符合条件的日志行。
      </p>
      <p v-else class="cpa-empty">
        网关已开启日志写入，但还没有写入日志行。
      </p>
    </template>
    <p v-else-if="!error" class="cpa-empty">未能读取 CPA 的日志开关状态。</p>
  </a-modal>
</template>

<style scoped>
.cpa-empty {
  color: var(--color-text-3);
  font-size: 12px;
  margin: 0;
  line-height: 1.65;
}
.cpa-log-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
  color: var(--color-text-3);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
.cpa-log-toolbar-end {
  justify-content: flex-end;
}
.cpa-log-filters {
  display: flex;
  gap: 8px;
  margin-bottom: 8px;
}
.cpa-log-filter {
  flex: 1;
}
.cpa-log-level-filter {
  width: 128px;
}
.cpa-log-lines {
  max-height: 55vh;
  overflow: auto;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
  padding: 4px 10px;
}
.cpa-log-line {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 2px 8px;
  padding: 4px 0;
  border-bottom: 1px solid var(--color-border-1);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.65;
  color: var(--color-text-2);
}
.cpa-log-line:last-child {
  border-bottom: 0;
}
.cpa-log-time {
  color: var(--color-text-3);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.cpa-log-request {
  padding: 0 5px;
  border: 1px solid var(--color-border-2);
  border-radius: 5px;
  background: var(--color-bg-2);
  color: var(--color-text-3);
  font: inherit;
  line-height: 1.5;
  cursor: pointer;
  transition:
    border-color 0.15s ease,
    color 0.15s ease;
}
.cpa-log-request:hover {
  border-color: var(--color-primary-light-4);
  color: var(--color-primary-6);
}
.cpa-log-request-none {
  border-color: transparent;
  background: none;
  cursor: default;
}
.cpa-log-level {
  min-width: 38px;
  text-align: center;
  text-transform: uppercase;
  font-size: 10px;
  letter-spacing: 0.04em;
}
.cpa-log-level.is-error,
.cpa-log-level.is-fatal,
.cpa-log-level.is-panic {
  color: var(--color-danger-6);
}
.cpa-log-level.is-warn {
  color: var(--color-warning-6);
}
.cpa-log-level.is-info {
  color: var(--color-primary-6);
}
.cpa-log-level.is-debug,
.cpa-log-level.is-trace {
  color: var(--color-text-3);
}
.cpa-log-source {
  color: var(--color-text-3);
  white-space: nowrap;
}
.cpa-log-message {
  flex: 1 1 320px;
  min-width: 0;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}
.cpa-log-back {
  margin-bottom: 10px;
}
.cpa-log-content {
  max-height: 55vh;
  margin: 0;
  padding: 13px;
  overflow: auto;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
  color: var(--color-text-2);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.65;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
