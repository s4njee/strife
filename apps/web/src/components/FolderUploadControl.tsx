import { For, Show, createSignal } from 'solid-js'
import { useUploads } from '../uploads/UploadContext'
import type { FolderUploadResult } from '../uploads/folderUpload'
import './FolderUploadControl.css'

interface FolderUploadControlProps {
  folderId: string
  onComplete: () => void | Promise<void>
}

export function FolderUploadControl(props: FolderUploadControlProps) {
  let input!: HTMLInputElement
  const uploads = useUploads()
  const [uploading, setUploading] = createSignal(false)
  const [results, setResults] = createSignal<FolderUploadResult[]>([])

  const chooseFolder = () => input.click()
  const handleSelection = async () => {
    const files = [...(input.files ?? [])]
    input.value = ''
    if (files.length === 0) return

    setUploading(true)
    setResults([])
    const completed = await uploads.start(
      files.map((file) => ({
        file,
        relativePath: file.webkitRelativePath || file.name,
      })),
      props.folderId,
      props.onComplete,
    )
    setResults(completed)
    setUploading(false)
  }

  const succeeded = () => results().filter((result) => result.node).length
  const failures = () => results().filter((result) => result.error)

  return (
    <div class="folder-upload">
      <input
        ref={(element) => {
          input = element
          element.setAttribute('webkitdirectory', '')
        }}
        class="folder-upload__input"
        type="file"
        multiple
        aria-label="Choose a folder to upload"
        onChange={() => void handleSelection()}
      />
      <button type="button" disabled={uploading()} onClick={chooseFolder}>
        {uploading() ? 'Uploading folder…' : 'Upload folder'}
      </button>
      <Show when={results().length > 0}>
        <div class="folder-upload__report" role="status">
          <p>
            {succeeded()} of {results().length} files uploaded
          </p>
          <Show when={failures().length > 0}>
            <ul>
              <For each={failures()}>
                {(result) => (
                  <li>
                    <strong>{result.path}:</strong> {result.error}
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </div>
      </Show>
    </div>
  )
}
