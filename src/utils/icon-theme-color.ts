const iconColors = new Map<string, string | null>();
const SAMPLE_SIZE = 32;

/** Sample the loaded local icon once. Transparent and neutral pixels do not
 * contribute a brand color; monochrome icons keep the theme-aware fallback. */
export function iconThemeColor(icon: HTMLImageElement): string | null {
  const source = icon.currentSrc || icon.src;
  if (iconColors.has(source)) return iconColors.get(source) ?? null;
  if (!icon.complete || !icon.naturalWidth || !icon.naturalHeight) return null;

  let color: string | null = null;
  try {
    const canvas = document.createElement("canvas");
    canvas.width = SAMPLE_SIZE;
    canvas.height = SAMPLE_SIZE;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) {
      iconColors.set(source, null);
      return null;
    }
    context.drawImage(icon, 0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
    const pixels = context.getImageData(0, 0, SAMPLE_SIZE, SAMPLE_SIZE).data;
    const buckets = new Map<number, { weight: number; red: number; green: number; blue: number }>();
    for (let offset = 0; offset < pixels.length; offset += 4) {
      const red = pixels[offset];
      const green = pixels[offset + 1];
      const blue = pixels[offset + 2];
      const alpha = pixels[offset + 3];
      if (alpha < 64 || Math.max(red, green, blue) - Math.min(red, green, blue) < 24) continue;
      // Group nearby colors so a gradient produces its dominant color instead
      // of averaging unrelated colors into grey. Weight antialiasing by alpha.
      const key = ((red >> 4) << 8) | ((green >> 4) << 4) | (blue >> 4);
      const bucket = buckets.get(key) ?? { weight: 0, red: 0, green: 0, blue: 0 };
      bucket.weight += alpha;
      bucket.red += red * alpha;
      bucket.green += green * alpha;
      bucket.blue += blue * alpha;
      buckets.set(key, bucket);
    }
    const dominant = [...buckets.values()].sort((left, right) => right.weight - left.weight)[0];
    if (dominant) {
      color = `#${[dominant.red, dominant.green, dominant.blue]
        .map((channel) => Math.round(channel / dominant.weight).toString(16).padStart(2, "0"))
        .join("")}`;
    }
  } catch {
    // An unreadable image must not block the card or its normal brand fallback.
  }
  iconColors.set(source, color);
  return color;
}
