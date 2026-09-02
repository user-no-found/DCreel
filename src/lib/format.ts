export function formatBytes(bytes: number | null): string {
  if (bytes === null) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

export function extensionLabel(extension: string | null, isDir: boolean): string {
  if (isDir) return "文件夹";
  return extension?.toUpperCase() || "文件";
}

export function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) || path;
}
