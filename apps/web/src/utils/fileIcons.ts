import type { FolderItemKind } from '../api/types'

export interface FileIconInfo {
  code: string
  color: string
  bg: string
  isFolder: boolean
}

export function getFileIconInfo(
  name?: string,
  kind?: FolderItemKind,
): FileIconInfo {
  if (kind === 'folder') {
    return {
      code: 'FLD',
      color: 'var(--color-folder-glyph, #7d8794)',
      bg: 'var(--color-folder-glyph-bg, color-mix(in oklab, var(--color-surface-raised) 80%, var(--color-border-row)))',
      isFolder: true,
    }
  }

  const ext = name ? (name.split('.').pop()?.toLowerCase() ?? '') : ''

  switch (ext) {
    case 'pdf':
      return {
        code: 'PDF',
        color: '#c0553f',
        bg: 'color-mix(in oklab, #c0553f 15%, transparent)',
        isFolder: false,
      }
    case 'doc':
    case 'docx':
    case 'txt':
    case 'rtf':
    case 'pages':
    case 'odt':
      return {
        code:
          ext.length <= 4 && ext !== 'docx' && ext !== 'pages'
            ? ext.toUpperCase()
            : 'DOC',
        color: '#3f6fc0',
        bg: 'color-mix(in oklab, #3f6fc0 15%, transparent)',
        isFolder: false,
      }
    case 'png':
    case 'jpg':
    case 'jpeg':
    case 'gif':
    case 'webp':
    case 'heic':
    case 'nef':
    case 'tiff':
    case 'bmp':
    case 'svg':
    case 'raw':
    case 'dng':
    case 'cr2':
    case 'arw':
      return {
        code: ext.length <= 4 && ext !== 'jpeg' ? ext.toUpperCase() : 'IMG',
        color: '#c08a3f',
        bg: 'color-mix(in oklab, #c08a3f 15%, transparent)',
        isFolder: false,
      }
    case 'xls':
    case 'xlsx':
    case 'csv':
    case 'numbers':
    case 'ods':
      return {
        code:
          ext.length <= 4 && ext !== 'xlsx' && ext !== 'numbers'
            ? ext.toUpperCase()
            : 'XLS',
        color: '#3f8f5c',
        bg: 'color-mix(in oklab, #3f8f5c 15%, transparent)',
        isFolder: false,
      }
    case 'fig':
    case 'ai':
    case 'psd':
    case 'sketch':
    case 'indd':
      return {
        code: ext.length <= 4 && ext !== 'sketch' ? ext.toUpperCase() : 'FIG',
        color: '#7a55c0',
        bg: 'color-mix(in oklab, #7a55c0 15%, transparent)',
        isFolder: false,
      }
    case 'md':
    case 'markdown':
      return {
        code: 'MD',
        color: '#5c6570',
        bg: 'color-mix(in oklab, #5c6570 15%, transparent)',
        isFolder: false,
      }
    case 'zip':
    case 'tar':
    case 'gz':
    case '7z':
    case 'rar':
    case 'bz2':
    case 'xz':
      return {
        code: ext.length <= 4 ? ext.toUpperCase() : 'ZIP',
        color: '#8a7d3f',
        bg: 'color-mix(in oklab, #8a7d3f 15%, transparent)',
        isFolder: false,
      }
    case 'mp3':
    case 'wav':
    case 'm4a':
    case 'flac':
    case 'aac':
    case 'ogg':
    case 'wma':
      return {
        code: ext.length <= 4 ? ext.toUpperCase() : 'AUD',
        color: '#3fc0b5',
        bg: 'color-mix(in oklab, #3fc0b5 15%, transparent)',
        isFolder: false,
      }
    case 'mp4':
    case 'mov':
    case 'mkv':
    case 'avi':
    case 'webm':
    case 'm4v':
      return {
        code: ext.length <= 4 && ext !== 'webm' ? ext.toUpperCase() : 'VID',
        color: '#c0683f',
        bg: 'color-mix(in oklab, #c0683f 15%, transparent)',
        isFolder: false,
      }
    case 'ppt':
    case 'pptx':
    case 'key':
    case 'odp':
      return {
        code: ext.length <= 4 && ext !== 'pptx' ? ext.toUpperCase() : 'PPT',
        color: '#c03f5c',
        bg: 'color-mix(in oklab, #c03f5c 15%, transparent)',
        isFolder: false,
      }
    case 'json':
    case 'rs':
    case 'ts':
    case 'js':
    case 'jsx':
    case 'tsx':
    case 'html':
    case 'css':
    case 'py':
    case 'sql':
    case 'toml':
    case 'yaml':
    case 'yml':
    case 'xml':
    case 'sh':
      return {
        code: ext.length <= 4 ? ext.toUpperCase() : 'CODE',
        color: '#557ac0',
        bg: 'color-mix(in oklab, #557ac0 15%, transparent)',
        isFolder: false,
      }
    default: {
      const code = ext && ext.length <= 4 ? ext.toUpperCase() : 'FILE'
      return {
        code,
        color: '#8a8f99',
        bg: 'color-mix(in oklab, #8a8f99 15%, transparent)',
        isFolder: false,
      }
    }
  }
}
