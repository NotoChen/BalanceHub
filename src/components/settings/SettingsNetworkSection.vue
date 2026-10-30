<script setup lang="ts">
import { IconWifi } from "@arco-design/web-vue/es/icon";
import type { AppSettings } from "../../stores/providers";
import { proxyModeOptions } from "../../utils/proxy-options";

defineProps<{ settings: AppSettings }>();
</script>

<template>
  <div class="settings-page settings-network-page">
    <section class="settings-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconWifi /></span>
        <div>
          <strong>网络代理</strong>
          <span>中转站请求、Webhook、更新检查和命令行都使用同一份代理设置。</span>
        </div>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>代理策略</strong>
          </div>
          <a-select v-model="settings.proxyMode" :options="proxyModeOptions" />
        </div>
        <div v-if="settings.proxyMode === 'custom'" class="settings-setting-row settings-setting-row-wide">
          <div class="settings-setting-copy">
            <strong>代理地址</strong>
          </div>
          <a-input
            v-model="settings.proxyUrl"
            placeholder="http://127.0.0.1:6152"
            title="支持 HTTP、HTTPS 和 SOCKS5 地址"
          />
        </div>
        <div class="settings-setting-row settings-setting-row-wide">
          <div class="settings-setting-copy">
            <strong>不走代理的地址</strong>
            <span>每行或逗号分隔一个域名；本机地址始终直连，无需重复填写。</span>
          </div>
          <a-textarea
            v-model="settings.proxyBypass"
            :auto-size="{ minRows: 2, maxRows: 6 }"
            placeholder="*.internal.example.com"
            aria-label="不走代理的地址"
          />
        </div>
        <div class="settings-setting-row settings-setting-row-field">
          <div class="settings-setting-copy">
            <strong>请求超时</strong>
            <span>单次额度刷新、签到等请求的最长等待时间。</span>
          </div>
          <a-input-number v-model="settings.httpTimeoutSecs" :min="5" :max="300" :step="5" aria-label="请求超时">
            <template #suffix>秒</template>
          </a-input-number>
        </div>
      </div>
    </section>
  </div>
</template>
