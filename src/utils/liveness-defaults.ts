export const DEFAULT_LIVENESS_MODEL = "gpt-5.5";
export const DEFAULT_LIVENESS_FIXED_PROMPT = "Explain: ls -la";
export const MIN_LIVENESS_INTERVAL_SECONDS = 1;
export const DEFAULT_LIVENESS_INTERVAL = 5 * 60;
export const DEFAULT_LIVENESS_RANDOM_MIN_INTERVAL = DEFAULT_LIVENESS_INTERVAL;
export const DEFAULT_LIVENESS_TIMEOUT = 75;
export const DEFAULT_LIVENESS_NUMBER_MIN = 2;
export const DEFAULT_LIVENESS_NUMBER_MAX = 97;

export interface LivenessTiming {
  livenessInterval: number;
  livenessRandomMinInterval: number;
  livenessRandomMaxInterval: number;
}

export interface ProviderLivenessTiming {
  interval: number;
  randomMinInterval: number;
  randomMaxInterval: number;
}

export function normalizeLivenessTiming<T extends LivenessTiming | ProviderLivenessTiming>(value: T): T {
  if ("interval" in value) {
    value.interval = Math.max(MIN_LIVENESS_INTERVAL_SECONDS, Number(value.interval) || 0);
    value.randomMinInterval = Math.max(
      MIN_LIVENESS_INTERVAL_SECONDS,
      Number(value.randomMinInterval) || 0,
    );
    value.randomMaxInterval = Math.max(
      value.randomMinInterval,
      Number(value.randomMaxInterval) || 0,
    );
    return value;
  }

  value.livenessInterval = Math.max(MIN_LIVENESS_INTERVAL_SECONDS, Number(value.livenessInterval) || 0);
  value.livenessRandomMinInterval = Math.max(
    MIN_LIVENESS_INTERVAL_SECONDS,
    Number(value.livenessRandomMinInterval) || 0,
  );
  value.livenessRandomMaxInterval = Math.max(
    value.livenessRandomMinInterval,
    Number(value.livenessRandomMaxInterval) || 0,
  );
  return value;
}

export const defaultLivenessPromptLibrary = () => [
  "Explain: {cmd}",
  "Convert to camelCase: {snake}",
  "Is this valid JSON: {json}",
  "Normalize path: {path}",
  "Fix typo: {typo}",
  "Rename variable: {var}",
  "Sum: {a}+{b}",
  "Choose smaller: {a} or {b}",
  "Make this concise: {sentence}",
  "Classify log level: {log}",
  "Title case: {phrase}",
  "Make slug: {phrase}",
  "Is this path absolute: {path}",
];

export const defaultLivenessPlaceholderPools = () => [
  { key: "word", values: ["folder", "record", "index", "config", "window", "button", "result", "summary"] },
  { key: "cmd", values: ["ls -la", "git status", "npm test", "node -v", "pwd", "cat README.md", "cargo check", "date"] },
  { key: "phrase", values: ["daily notes", "release plan", "window title", "build result", "local draft", "error summary"] },
  { key: "snake", values: ["file_name", "total_count", "last_seen", "item_index", "window_title", "retry_delay", "created_at"] },
  { key: "var", values: ["tmpValue", "rawText", "nextItem", "userName", "filePath", "totalCount", "retryCount"] },
  { key: "path", values: ["/tmp/../var/log", "./src/../README.md", "./notes//today.md", "~/Downloads/../.config", "/usr/local/../bin"] },
  { key: "json", values: ["{\"ok\":true}", "{\"name\":\"demo\"}", "{\"items\":[\"a\",\"b\"]}", "{\"enabled\":false}", "{\"limit\":3}"] },
  { key: "status", values: ["200", "201", "204", "301", "400", "404", "418"] },
  { key: "typo", values: ["recieve", "adress", "teh", "occured", "seperate", "enviroment"] },
  {
    key: "sentence",
    values: [
      "The file was saved after the last edit.",
      "This setting controls how often data refreshes.",
      "The window title should stay short.",
      "The table row needs a clearer label.",
      "The command finished without output.",
    ],
  },
  { key: "log", values: ["WARN retry after timeout", "ERROR file not found", "INFO build completed", "DEBUG cache hit", "WARN missing config"] },
  { key: "port", values: ["22", "80", "443", "3000", "5432", "6379", "8080"] },
];
