// Draft preferences only. Existence, reparse points and runtime enforcement
// must be checked by a future file-aware native engine before activation.
export function normalizeFolder(value: string, letter: string): string | null {
  const path = value.trim().replaceAll("/", "\\").replace(/\\+$/, "");
  if (path.length > 240 || !/^[a-z]:\\/i.test(path)) return null;
  if (path.slice(0, 2).toLowerCase() !== letter.toLowerCase()) return null;
  const parts = path.slice(3).split("\\");
  if (parts.some(part => !part || part === "." || part === ".." ||
    /[<>:"|?*\u0000-\u001f]/.test(part) || /[. ]$/.test(part))) return null;
  return letter.toUpperCase() + "\\" + parts.join("\\");
}

export function loadFolders(): Record<string, string[]> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem("superhdd-selected-folders") || "{}");
    if (!value || typeof value !== "object" || Array.isArray(value)) return {};
    return Object.fromEntries(Object.entries(value).filter(([key, paths]) =>
      key.length <= 512 && Array.isArray(paths) && paths.length <= 64 &&
      paths.every(path => typeof path === "string" && path.length <= 240)));
  } catch { return {}; }
}
