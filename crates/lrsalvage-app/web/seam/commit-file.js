export async function commitFile(handle, bytes, beforeCommit) {
  const stream = await handle.createWritable({ mode: 'exclusive' });
  try {
    await stream.write(bytes);
    await beforeCommit?.();
    await stream.close();
  } catch (error) {
    await stream.abort().catch(() => {});
    throw error;
  }
}
