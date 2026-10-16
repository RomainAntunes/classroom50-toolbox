import { describe, expect, it } from "vitest";
import { cloneTarget } from "./clone";

describe("cloneTarget", () => {
  it("joins with the parent's own separator", () => {
    expect(cloneTarget("/Users/ana/classes", "cs50-hello-ana")).toBe(
      "/Users/ana/classes/cs50-hello-ana",
    );
    expect(cloneTarget("C:\\Users\\ana\\classes", "lab")).toBe("C:\\Users\\ana\\classes\\lab");
  });

  it("drops trailing separators", () => {
    expect(cloneTarget("/Users/ana/classes/", "lab")).toBe("/Users/ana/classes/lab");
    expect(cloneTarget("C:\\classes\\", "lab")).toBe("C:\\classes\\lab");
    expect(cloneTarget("/", "lab")).toBe("/lab");
  });
});
