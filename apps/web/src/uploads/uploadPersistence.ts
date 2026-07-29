const DATABASE_NAME = 'strife-uploads'
const STORE_NAME = 'files'

export async function persistUploadFile(
  sessionId: string,
  file: File,
): Promise<void> {
  const database = await openDatabase()
  await transactionPromise(
    database
      .transaction(STORE_NAME, 'readwrite')
      .objectStore(STORE_NAME)
      .put(file, sessionId),
  )
  database.close()
}

export async function loadUploadFile(
  sessionId: string,
): Promise<File | undefined> {
  const database = await openDatabase()
  const file = await transactionPromise<File | undefined>(
    database.transaction(STORE_NAME).objectStore(STORE_NAME).get(sessionId),
  )
  database.close()
  return file
}

export async function removeUploadFile(sessionId: string): Promise<void> {
  const database = await openDatabase()
  await transactionPromise(
    database
      .transaction(STORE_NAME, 'readwrite')
      .objectStore(STORE_NAME)
      .delete(sessionId),
  )
  database.close()
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE_NAME, 1)
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE_NAME)) {
        request.result.createObjectStore(STORE_NAME)
      }
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function transactionPromise<T = undefined>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}
