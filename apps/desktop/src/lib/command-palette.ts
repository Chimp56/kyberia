export function moveCommandSelection(length: number, current: number, direction: 1 | -1): number {
  if (length === 0) return -1;
  return (current + direction + length) % length;
}

export function clampCommandSelection(length: number, current: number): number {
  if (length === 0) return -1;
  return Math.min(Math.max(current, 0), length - 1);
}
