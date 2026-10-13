/** A caller-selected owner group applies only to GitHub object transport. */
export function validateExportGroup(group: string, backend: string, mode: string): string {
  if (!group) return ''
  if (backend !== 'github' || mode !== 'objects') {
    throw new Error('export-group requires backend github and github-cache-mode objects')
  }
  if (group.length > 256 || !/^[a-z0-9]/.test(group) || /[^a-z0-9._-]/.test(group)) {
    throw new Error('export-group must be 1-256 lowercase ASCII letters, digits, dots, underscores, or hyphens; start with a letter or digit')
  }
  return group
}
