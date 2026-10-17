/** A readable message from whatever a bridge call rejected with: Tauri rejects with the command's error value (a
 * string here); the mock rejects with an Error. */
export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return 'Unknown error';
}
