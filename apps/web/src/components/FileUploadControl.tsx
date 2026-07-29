import { createSignal } from 'solid-js'
import { uploadFiles } from '../uploads/folderUpload'

interface FileUploadControlProps {
  folderId: string
  onComplete: () => void | Promise<void>
}

export function FileUploadControl(props: FileUploadControlProps) {
  let input!: HTMLInputElement
  const [uploading, setUploading] = createSignal(false)

  const handleSelection = async () => {
    const files = [...(input.files ?? [])]
    input.value = ''
    if (files.length === 0) return
    setUploading(true)
    const results = await uploadFiles(
      files.map((file) => ({ file, relativePath: file.name })),
      props.folderId,
    )
    setUploading(false)
    if (results.some((result) => result.node)) await props.onComplete()
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
        disabled={uploading()}
        onClick={() => input.click()}
      >
        {uploading() ? 'Uploading…' : 'Upload files'}
      </button>
    </div>
  )
}
