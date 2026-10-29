import { expect, test } from "vitest";
import { tokenFromHash } from "./auth";

test("reads the token from the URL fragment", () => {
  expect(tokenFromHash("#token=abc123")).toBe("abc123");
  expect(tokenFromHash("#other=1")).toBeNull();
  expect(tokenFromHash("")).toBeNull();
});
