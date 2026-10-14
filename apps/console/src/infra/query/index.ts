import { ApiError } from "@/infra/serializer";
import {
  QueryClient,
  QueryObserver,
  type QueryObserverOptions,
  type QueryObserverResult,
} from "@tanstack/query-core";
import type { ReactiveController, ReactiveControllerHost } from "lit";
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5000,
      retry: (count, error) => count < 1 && !(error instanceof ApiError && error.status < 500),
    },
    mutations: { retry: false },
  },
});
export class QueryController<T> implements ReactiveController {
  private observer: QueryObserver<T, Error>;
  private unsubscribe?: () => void;
  result: QueryObserverResult<T, Error>;
  constructor(
    private host: ReactiveControllerHost,
    options: QueryObserverOptions<T, Error>,
  ) {
    this.observer = new QueryObserver(queryClient, options);
    this.result = this.observer.getCurrentResult();
    host.addController(this);
  }
  hostConnected(): void {
    this.unsubscribe = this.observer.subscribe((result) => {
      this.result = result;
      this.host.requestUpdate();
    });
    this.result = this.observer.getCurrentResult();
  }
  hostDisconnected(): void {
    this.unsubscribe?.();
  }
  configure(options: QueryObserverOptions<T, Error>): void {
    this.observer.setOptions(options);
    this.result = this.observer.getCurrentResult();
  }
  refresh(): Promise<QueryObserverResult<T, Error>> {
    return this.observer.refetch();
  }
}
