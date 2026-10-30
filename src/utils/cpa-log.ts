/// One line of the gateway's own log. The formatter writes
/// `[time] [request-id] [level] [file:line] message`, so the leading brackets
/// are structure rather than text the user needs to read.
export interface CpaLogLine {
  raw: string;
  timestamp: string;
  /// Last eight characters of the request ID, empty when the line is not tied
  /// to one. The gateway accepts this shortened form for the drill-down.
  requestId: string;
  /// Normalized level, empty when the line carries none.
  level: string;
  source: string;
  message: string;
}

const PREFIX = /^\[([^\]]*)\] \[([^\]]*)\] \[([^\]]*)\]\s*/;
const SOURCE = /^\[([^\]]+:\d+)\]\s*/;
const REQUEST_ID = /^[0-9a-f]{8}$/i;
/// Every line without a request still carries eight dashes, which must not
/// become a drill-down target.
const NO_REQUEST_ID = /^-+$/;

export function parseCpaLogLine(raw: string): CpaLogLine {
  const text = raw.replace(/\r$/, "");
  const prefix = text.match(PREFIX);
  if (!prefix) {
    return {
      raw: text,
      timestamp: "",
      requestId: "",
      level: "",
      source: "",
      message: text,
    };
  }
  const request = prefix[2].trim();
  const rest = text.slice(prefix[0].length);
  const source = rest.match(SOURCE);
  return {
    raw: text,
    timestamp: prefix[1].trim(),
    requestId: REQUEST_ID.test(request) && !NO_REQUEST_ID.test(request) ? request : "",
    level: normalizeCpaLogLevel(prefix[3]),
    source: source ? source[1] : "",
    message: source ? rest.slice(source[0].length) : rest,
  };
}

/// Logrus pads the level to five columns and names `warning` as `warn`; the
/// panel groups by the level, so both spellings have to become one value.
export function normalizeCpaLogLevel(raw: string): string {
  const value = raw.trim().toLowerCase();
  if (!value) return "";
  if (value === "warning") return "warn";
  return /^[a-z]+$/.test(value) ? value : "";
}

/// Severity order, so the filter lists errors before debug lines instead of
/// following the order the buffer happens to hold.
const LEVEL_ORDER = ["fatal", "panic", "error", "warn", "info", "debug", "trace"];

export function cpaLogLevels(levels: Iterable<string>): string[] {
  return [...new Set(levels)]
    .filter(Boolean)
    .sort((left, right) => {
      const a = LEVEL_ORDER.indexOf(left);
      const b = LEVEL_ORDER.indexOf(right);
      // An unknown level still gets a stable place after the known ones.
      return (a === -1 ? LEVEL_ORDER.length : a) - (b === -1 ? LEVEL_ORDER.length : b) ||
        left.localeCompare(right);
    });
}
