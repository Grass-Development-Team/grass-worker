import { MutationCache, QueryCache, QueryClient } from "@tanstack/react-query";

import { showErrorToast } from "./toast";

export const queryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (cause, query) => showErrorToast(cause, `query-error:${query.queryHash}`),
  }),
  mutationCache: new MutationCache({
    onError: (cause) => showErrorToast(cause),
  }),
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      retry: 1,
    },
  },
});
