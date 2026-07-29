import { useParams } from '@solidjs/router'

interface WorkspaceViewProps {
  eyebrow: string
  title: string
  description: string
}

function WorkspaceView(props: WorkspaceViewProps) {
  return (
    <section class="workspace-view">
      <header>
        <p class="workspace-view__eyebrow">{props.eyebrow}</p>
        <h1>{props.title}</h1>
        <p class="workspace-view__description">{props.description}</p>
      </header>
      <div class="workspace-view__surface" />
    </section>
  )
}

export function RootFolderView() {
  return (
    <WorkspaceView
      eyebrow="Library"
      title="All Files"
      description="Everything stored in your Strife drive."
    />
  )
}

export function FolderView() {
  const params = useParams<{ id: string }>()
  return (
    <WorkspaceView
      eyebrow="Folder"
      title="Folder"
      description={`Folder ${params.id}`}
    />
  )
}

export function FavoritesView() {
  return (
    <WorkspaceView
      eyebrow="Library"
      title="Favorites"
      description="Frequently used files and folders will appear here."
    />
  )
}

export function TrashView() {
  return (
    <WorkspaceView
      eyebrow="Library"
      title="Trash"
      description="Deleted items are retained for 30 days."
    />
  )
}
