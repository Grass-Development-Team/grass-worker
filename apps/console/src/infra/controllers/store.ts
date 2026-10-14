import type { Store } from "@/infra/state";
import type { ReactiveController, ReactiveControllerHost } from "lit";
export class StoreController<T> implements ReactiveController {
  private unsubscribe?: () => void;
  constructor(
    private host: ReactiveControllerHost,
    public store: Store<T>,
  ) {
    host.addController(this);
  }
  hostConnected(): void {
    this.unsubscribe = this.store.subscribe(() => this.host.requestUpdate());
  }
  hostDisconnected(): void {
    this.unsubscribe?.();
  }
  get value(): T {
    return this.store.value;
  }
}
