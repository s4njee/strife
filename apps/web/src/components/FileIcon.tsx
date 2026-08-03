import type { FolderItemKind } from '../api/types'
import { getFileIconInfo } from '../utils/fileIcons'
import './FileIcon.css'

interface FileIconProps {
  name?: string
  kind?: FolderItemKind
  class?: string
}

export function FileIcon(props: FileIconProps) {
  const info = () => getFileIconInfo(props.name, props.kind)
  return (
    <span
      class={`file-icon ${props.class ?? ''}`}
      data-kind={props.kind}
      style={{
        color: info().color,
        background: info().bg,
      }}
      aria-hidden="true"
    >
      {info().code}
    </span>
  )
}
