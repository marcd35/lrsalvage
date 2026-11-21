export async function commitFile(handle, bytes, beforeWrite) {
  const stream = await handle.createWritable({ mode: 'exclusive' });
  try {
    await beforeWrite?.();
    await stream.write(bytes);
    await stream.close();
  } catch (error) {
    await stream.abort().catch(() => {});
    throw error;
  }
}
