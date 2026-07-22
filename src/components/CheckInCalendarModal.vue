<script setup lang="ts">
import { computed } from "vue";
import { IconCalendarClock, IconLeft, IconRefresh, IconRight } from "@arco-design/web-vue/es/icon";
import type { Provider, ProviderCheckInRecord, ProviderCheckInRecordsResult } from "../stores/providers";
import { formatQuotaValue } from "../utils/provider-display";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  month: string;
  loading: boolean;
  result: ProviderCheckInRecordsResult | null;
  error: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:month": [month: string];
  refresh: [];
}>();

const weekDays = ["日", "一", "二", "三", "四", "五", "六"];

const modalTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 签到记录` : "签到记录",
);

const recordByDate = computed(() => {
  const map = new Map<string, ProviderCheckInRecord>();
  for (const record of props.result?.records ?? []) {
    map.set(record.date, record);
  }
  return map;
});

const calendarDays = computed(() => {
  const [year, month] = parseMonth(props.month);
  const firstDay = new Date(year, month - 1, 1);
  const daysInMonth = new Date(year, month, 0).getDate();
  const startOffset = firstDay.getDay();
  const cells: {
    key: string;
    date: string;
    day: number | null;
    record: ProviderCheckInRecord | null;
    today: boolean;
  }[] = [];
  const today = localDateKey(new Date());

  for (let index = 0; index < startOffset; index += 1) {
    cells.push({ key: `empty-start-${index}`, date: "", day: null, record: null, today: false });
  }
  for (let day = 1; day <= daysInMonth; day += 1) {
    const date = `${props.month}-${String(day).padStart(2, "0")}`;
    cells.push({
      key: date,
      date,
      day,
      record: recordByDate.value.get(date) ?? null,
      today: date === today,
    });
  }
  while (cells.length % 7 !== 0) {
    cells.push({ key: `empty-end-${cells.length}`, date: "", day: null, record: null, today: false });
  }
  return cells;
});

const checkedDays = computed(() => props.result?.records.length ?? 0);

const quotaTotal = computed(() =>
  (props.result?.records ?? []).reduce((total, record) => total + (record.quotaDelta ?? 0), 0),
);

const todayRecord = computed(() => recordByDate.value.get(localDateKey(new Date())) ?? null);

const monthTitle = computed(() => {
  const [year, month] = parseMonth(props.month);
  return `${year} 年 ${month} 月`;
});

function parseMonth(value: string) {
  const [yearRaw, monthRaw] = value.split("-");
  const year = Number(yearRaw) || new Date().getFullYear();
  const month = Number(monthRaw) || new Date().getMonth() + 1;
  return [year, month] as const;
}

function localDateKey(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function shiftMonth(offset: number) {
  const [year, month] = parseMonth(props.month);
  const date = new Date(year, month - 1 + offset, 1);
  emit("update:month", `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`);
}

function quotaLabel(record: ProviderCheckInRecord) {
  if (record.quotaDelta === null || record.quotaDelta === undefined) {
    return "已签到";
  }
  const display = props.result?.quotaDisplay ?? {
    quotaDisplayType: props.provider?.quota.displayType || "currency",
    currencySymbol: props.provider?.quota.currencySymbol || "$",
  };
  return `+${formatQuotaValue(record.quotaDelta, display)}`;
}

function dayTitle(record: ProviderCheckInRecord | null) {
  if (!record) return "";
  const parts = [quotaLabel(record)];
  if (record.message.trim()) {
    parts.push(record.message.trim());
  }
  return parts.join(" · ");
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal checkin-calendar-modal"
    :footer="false"
    :width="760"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title checkin-calendar-title">
        <span class="surface-modal-title-icon"><icon-calendar-clock /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ modalTitle }}</strong>
        </span>
      </div>
    </template>
    <div class="checkin-calendar-panel">
      <div class="checkin-calendar-toolbar">
        <a-button-group>
          <a-button @click="shiftMonth(-1)">
            <template #icon><icon-left /></template>
          </a-button>
          <a-button @click="shiftMonth(1)">
            <template #icon><icon-right /></template>
          </a-button>
        </a-button-group>
        <strong>{{ monthTitle }}</strong>
        <a-button :loading="loading" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新
        </a-button>
      </div>

      <a-alert v-if="error" type="warning" :content="error" />

      <a-spin :loading="loading">
        <div class="checkin-calendar-summary">
          <div>
            <span>签到天数</span>
            <strong>{{ checkedDays }}</strong>
          </div>
          <div>
            <span>累计增加</span>
            <strong>
              {{
                result
                  ? formatQuotaValue(quotaTotal, result.quotaDisplay)
                    : formatQuotaValue(0, {
                      quotaDisplayType: provider?.quota.displayType || "currency",
                      currencySymbol: provider?.quota.currencySymbol || "$",
                    })
              }}
            </strong>
          </div>
          <div>
            <span>今日奖励</span>
            <strong>{{ todayRecord ? quotaLabel(todayRecord) : "-" }}</strong>
          </div>
        </div>

        <div class="checkin-calendar-grid">
          <div v-for="day in weekDays" :key="day" class="checkin-calendar-weekday">{{ day }}</div>
          <div
            v-for="cell in calendarDays"
            :key="cell.key"
            class="checkin-calendar-day"
            :class="{
              'checkin-calendar-day-empty': !cell.day,
              'checkin-calendar-day-checked': Boolean(cell.record),
              'checkin-calendar-day-today': cell.today,
            }"
            :title="dayTitle(cell.record)"
          >
            <template v-if="cell.day">
              <span>{{ cell.day }}</span>
              <strong v-if="cell.record">{{ quotaLabel(cell.record) }}</strong>
            </template>
          </div>
        </div>
      </a-spin>
    </div>
  </a-modal>
</template>
