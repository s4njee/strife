import { createSignal } from 'solid-js'
import { useUploads } from '../uploads/UploadContext'

interface FileUploadControlProps {
  folderId: string
  onComplete: () => void | Promise<void>
}

export function FileUploadControl(props: FileUploadControlProps) {
  let input!: HTMLInputElement
  const uploads = useUploads()
  const [uploading, setUploading] = createSignal(false)

  const handleSelection = async () => {
    const files = [...(input.files ?? [])]
    input.value = ''
    if (files.length === 0) return
    setUploading(true)
    await uploads.start(
      files.map((file) => ({ file, relativePath: file.name })),
      props.folderId,
      props.onComplete,
    )
    setUploading(false)
  }

  return (
    <div class="folder-upload">
      <input
        ref={input}
        class="folder-upload__input"
        type="file"
        multiple
        aria-label="Choose files to upload"
        onChange={() => void handleSelection()}
      />
      <button
        type="button"
        class="btn--primary"
        disabled={uploading()}
        onClick={() => input.click()}
      >
        {uploading() ? 'Uploading…' : '+ Upload'}
      </button>
    </div>
  )
}
