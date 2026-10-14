export class Store<T> {
  private listeners = new Set<() => void>();
  constructor(public value: T) {}
  set(value: T): void {
    this.value = value;
    this.listeners.forEach((listener) => listener());
  }
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
}
