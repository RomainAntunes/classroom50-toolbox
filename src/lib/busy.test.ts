import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { atLeast, MIN_BUSY_MS } from "./busy.js";

describe("atLeast", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /** Settles `promise` and reports when, in fake milliseconds. */
  function track<T>(promise: Promise<T>) {
    const state: { at: number | null; value?: T; error?: unknown } = { at: null };
    promise.then(
      (value) => Object.assign(state, { at: Date.now(), value }),
      (error) => Object.assign(state, { at: Date.now(), error }),
    );
    return state;
  }

  it("holds a quick answer until the minimum has passed", async () => {
    const start = Date.now();
    const state = track(atLeast(Promise.resolve(42)));
    await vi.advanceTimersByTimeAsync(MIN_BUSY_MS - 1);
    expect(state.at).toBeNull();
    await vi.advanceTimersByTimeAsync(1);
    expect(state.at! - start).toBe(MIN_BUSY_MS);
    expect(state.value).toBe(42);
  });

  it("rejects with the work's error, after the minimum too", async () => {
    const start = Date.now();
    const error = new Error("nope");
    const state = track(atLeast(Promise.reject(error)));
    await vi.advanceTimersByTimeAsync(MIN_BUSY_MS - 1);
    expect(state.at).toBeNull();
    await vi.advanceTimersByTimeAsync(1);
    expect(state.at! - start).toBe(MIN_BUSY_MS);
    expect(state.error).toBe(error);
  });

  it("adds nothing to slower work", async () => {
    const start = Date.now();
    const slow = new Promise((resolve) => setTimeout(() => resolve("done"), 500));
    const state = track(atLeast(slow));
    await vi.advanceTimersByTimeAsync(500);
    expect(state.at! - start).toBe(500);
    expect(state.value).toBe("done");
  });
});
