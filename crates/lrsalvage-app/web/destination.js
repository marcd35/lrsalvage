function destination(operation, mode) {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open('lrsalvage-export', 1);
    open.onerror = () => reject(open.error);
    open.onupgradeneeded = () => open.result.createObjectStore('settings');
    open.onsuccess = () => {
      const db = open.result;
      const transaction = db.transaction('settings', mode);
      const request = operation(transaction.objectStore('settings'));
      transaction.oncomplete = () => {
        db.close();
        resolve(request.result);
      };
      transaction.onabort = () => {
        db.close();
        reject(transaction.error);
      };
    };
  });
}

export const readDestination = () => destination((store) => store.get('destination'), 'readonly');
export const rememberDestination = (handle) => destination(
  (store) => store.put(handle, 'destination'), 'readwrite',
);
