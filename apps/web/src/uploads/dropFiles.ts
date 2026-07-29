import type { UploadCandidate } from './folderUpload'

export async function collectDroppedFiles(
  transfer: DataTransfer,
): Promise<UploadCandidate[]> {
  const entries = [...transfer.items]
    .map((item) => item.webkitGetAsEntry?.())
    .filter(
      (entry): entry is FileSystemEntry =>
        entry !== null && entry !== undefined,
    )
  if (entries.length === 0) {
    return [...transfer.files].map((file) => ({
      file,
      relativePath: file.name,
    }))
  }
  const nested = await Promise.all(entries.map((entry) => walkEntry(entry, '')))
  return nested.flat()
}

async function walkEntry(
  entry: FileSystemEntry,
  parentPath: string,
): Promise<UploadCandidate[]> {
  const relativePath = parentPath ? `${parentPath}/${entry.name}` : entry.name
  if (entry.isFile) {
    const file = await readFile(entry as FileSystemFileEntry)
    return [{ file, relativePath }]
  }
  if (!entry.isDirectory) return []
  const children = await readDirectory(entry as FileSystemDirectoryEntry)
  const nested = await Promise.all(
    children.map((child) => walkEntry(child, relativePath)),
  )
  return nested.flat()
}

function readFile(entry: FileSystemFileEntry): Promise<File> {
  return new Promise((resolve, reject) => entry.file(resolve, reject))
}

async function readDirectory(
  entry: FileSystemDirectoryEntry,
): Promise<FileSystemEntry[]> {
  const reader = entry.createReader()
  const entries: FileSystemEntry[] = []
  while (true) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) =>
      reader.readEntries(resolve, reject),
    )
    if (batch.length === 0) return entries
    entries.push(...batch)
  }
}
