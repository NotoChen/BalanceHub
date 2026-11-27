import type { CSSProperties } from "vue";
import { agentCliVisuals } from "../agent-cli/visuals.ts";
import type { AgentCliKind } from "../stores/providers";

// Motion bounds are expressed per full card perimeter. The card's own hover
// beam is independent from these CLI icon motion settings.
export const PROVIDER_CARD_CLI_ORBIT_MIN_CYCLE_MS = 5_000;
export const PROVIDER_CARD_CLI_ORBIT_MAX_CYCLE_MS = 11_000;
export const PROVIDER_CARD_CLI_ORBIT_MIN_SPEED_PROGRESS_PER_SECOND = 4.5;
export const PROVIDER_CARD_CLI_ORBIT_MAX_SPEED_PROGRESS_PER_SECOND = 8.5;
export const PROVIDER_CARD_CLI_ORBIT_QUEUE_GAP_PROGRESS = 2.5;
export const PROVIDER_CARD_CLI_ORBIT_QUEUE_MAX_SPAN_PROGRESS = 60;
export const PROVIDER_CARD_CLI_ORBIT_QUEUE_HEAD_PROGRESS = 75;

export interface ProviderCardCliOrbitSpec {
  id: string;
  cliKind: AgentCliKind;
  color: string;
  glow?: string;
  title?: string;
}

export interface ProviderCardCliOrbitLayout extends ProviderCardCliOrbitSpec {
  style: CSSProperties;
  phaseProgress: number;
}

export interface ProviderCardCliOrbitMotion {
  progress: number;
  cycleElapsedMs: number;
  cycleDurationMs: number;
  speedProgressPerSecond: number;
}

export function providerCardCliOrbitSpec(
  cliKind: AgentCliKind,
  options: { id?: string; title?: string } = {},
): ProviderCardCliOrbitSpec {
  const visual = agentCliVisuals[cliKind];
  return {
    id: options.id || cliKind,
    cliKind,
    color: visual.orbitColor,
    glow: visual.orbitGlow,
    title: options.title,
  };
}

function normalizedProgress(progress: number) {
  return ((progress % 100) + 100) % 100;
}

function randomInRange(min: number, max: number, random: () => number) {
  return min + (max - min) * Math.min(1, Math.max(0, random()));
}

function quantizedRandom(
  min: number,
  max: number,
  quantum: number,
  random: () => number,
) {
  return Math.round(randomInRange(min, max, random) / quantum) * quantum;
}

function randomCycleDurationMs(random: () => number) {
  return quantizedRandom(
    PROVIDER_CARD_CLI_ORBIT_MIN_CYCLE_MS,
    PROVIDER_CARD_CLI_ORBIT_MAX_CYCLE_MS,
    250,
    random,
  );
}

function randomSpeedProgressPerSecond(random: () => number) {
  return quantizedRandom(
    PROVIDER_CARD_CLI_ORBIT_MIN_SPEED_PROGRESS_PER_SECOND,
    PROVIDER_CARD_CLI_ORBIT_MAX_SPEED_PROGRESS_PER_SECOND,
    0.1,
    random,
  );
}

export function createProviderCardCliOrbitMotion(
  progress: number,
  random = Math.random,
): ProviderCardCliOrbitMotion {
  return {
    progress: normalizedProgress(progress),
    cycleElapsedMs: 0,
    cycleDurationMs: randomCycleDurationMs(random),
    speedProgressPerSecond: randomSpeedProgressPerSecond(random),
  };
}

export function advanceProviderCardCliOrbitMotion(
  motion: ProviderCardCliOrbitMotion,
  deltaMs: number,
  random = Math.random,
) {
  let remainingMs = Math.max(0, deltaMs);
  let progressDelta = 0;

  while (remainingMs > 0) {
    const cycleRemainingMs = motion.cycleDurationMs - motion.cycleElapsedMs;
    const stepMs = Math.min(remainingMs, cycleRemainingMs);
    progressDelta += (motion.speedProgressPerSecond * stepMs) / 1000;
    motion.cycleElapsedMs += stepMs;
    remainingMs -= stepMs;

    if (motion.cycleElapsedMs >= motion.cycleDurationMs) {
      motion.cycleElapsedMs = 0;
      motion.cycleDurationMs = randomCycleDurationMs(random);
      motion.speedProgressPerSecond = randomSpeedProgressPerSecond(random);
    }
  }

  motion.progress = normalizedProgress(motion.progress + progressDelta);
  return motion;
}

type OrbitFrameSubscriber = (now: number) => void;

/// 轨道是 5–11 秒一轮的慢速位移，30fps 已经足够平滑；把逐帧回调减半，
/// 卡片较多时持续写入 CSS 变量的开销也随之减半。
const ORBIT_FRAME_MIN_INTERVAL_MS = 1000 / 30;

const orbitFrameSubscribers = new Set<OrbitFrameSubscriber>();
let orbitFrameHandle: number | null = null;
let lastOrbitDispatchAt = 0;

function dispatchOrbitFrame(now: number) {
  orbitFrameHandle = null;
  // 半毫秒容差：60Hz 的帧间隔是 16.67ms，正好卡在两帧阈值上时浮点误差
  // 会让本该触发的帧被跳过，实际降到 20fps。
  if (now - lastOrbitDispatchAt >= ORBIT_FRAME_MIN_INTERVAL_MS - 0.5) {
    lastOrbitDispatchAt = now;
    for (const subscriber of [...orbitFrameSubscribers]) {
      subscriber(now);
    }
  }
  scheduleOrbitFrame();
}

function scheduleOrbitFrame() {
  if (
    orbitFrameHandle !== null ||
    orbitFrameSubscribers.size === 0 ||
    typeof requestAnimationFrame === "undefined"
  ) {
    return;
  }
  orbitFrameHandle = requestAnimationFrame(dispatchOrbitFrame);
}

export function subscribeProviderCardCliOrbitFrames(
  subscriber: OrbitFrameSubscriber,
) {
  if (typeof requestAnimationFrame === "undefined") return () => undefined;

  orbitFrameSubscribers.add(subscriber);
  scheduleOrbitFrame();

  return () => {
    orbitFrameSubscribers.delete(subscriber);
    if (orbitFrameSubscribers.size === 0 && orbitFrameHandle !== null) {
      cancelAnimationFrame(orbitFrameHandle);
      orbitFrameHandle = null;
    }
  };
}

function queueGapProgress(count: number) {
  if (count <= 1) return 0;
  return Math.min(
    PROVIDER_CARD_CLI_ORBIT_QUEUE_GAP_PROGRESS,
    PROVIDER_CARD_CLI_ORBIT_QUEUE_MAX_SPAN_PROGRESS / (count - 1),
  );
}

export function layoutProviderCardCliOrbits(
  specs: readonly ProviderCardCliOrbitSpec[],
): ProviderCardCliOrbitLayout[] {
  if (specs.length === 0) return [];

  const gapProgress = queueGapProgress(specs.length);
  return specs.map((spec, index) => {
    // Keep agents in one compact train. The head stays fixed, so new agents
    // append at the tail without shifting the existing icons.
    const phaseProgress = normalizedProgress(
      PROVIDER_CARD_CLI_ORBIT_QUEUE_HEAD_PROGRESS - index * gapProgress,
    );
    return {
      ...spec,
      phaseProgress,
      style: {
        "--provider-card-cli-orbit-progress": `${phaseProgress}%`,
        "--provider-card-cli-orbit-color": spec.color,
        "--provider-card-cli-orbit-glow": spec.glow || spec.color,
      } as CSSProperties,
    };
  });
}
