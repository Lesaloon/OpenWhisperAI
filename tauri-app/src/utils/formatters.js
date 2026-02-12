export function formatBytes(value) {
  if (value === 0) return "0 B";
  const base = 1024;
  const units = ["B", "KB", "MB", "GB", "TB"];
  const magnitude = Math.floor(Math.log(value) / Math.log(base));
  const scaled = value / Math.pow(base, magnitude);
  return `${scaled.toFixed(magnitude === 0 ? 0 : 1)} ${units[magnitude]}`;
}

export function formatDuration(seconds) {
  if (seconds <= 0) return "0s";
  const minutes = Math.floor(seconds / 60);
  const remaining = Math.floor(seconds % 60);
  if (minutes === 0) return `${remaining}s`;
  return `${minutes}m ${remaining.toString().padStart(2, "0")}s`;
}

export function formatPercent(value) {
  const clamped = Math.min(100, Math.max(0, value));
  return `${clamped}%`;
}

export function formatRate(bytesPerSecond) {
  if (bytesPerSecond <= 0) return "-";
  return `${formatBytes(bytesPerSecond)}/s`;
}
