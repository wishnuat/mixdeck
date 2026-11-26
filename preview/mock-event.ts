import { subscribe } from "./mock-state";

export async function listen<T>(event: string, cb: (e: { payload: T }) => void) {
  return subscribe(event, (payload) => cb({ payload: payload as T }));
}
