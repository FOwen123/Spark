import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Conn, type ConnStatus } from "./conn";

/** A socket the test opens and closes by hand. */
class FakeSocket {
  static OPEN = 1;
  static all: FakeSocket[] = [];
  readyState = 0;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  constructor() {
    FakeSocket.all.push(this);
  }
  send(m: string) {
    this.sent.push(m);
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  close() {
    this.readyState = 3;
    this.onclose?.();
  }
}

beforeEach(() => {
  FakeSocket.all = [];
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", FakeSocket);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("a request that failed while offline is not sent after reconnecting", async () => {
  const conn = new Conn("ws://spark");
  const seen: ConnStatus[] = [];
  conn.onStatus((s) => seen.push(s));
  FakeSocket.all[0].open();
  FakeSocket.all[0].close();

  const lost = conn.request({ t: "do", cmd: { type: "delete" } });
  const outcome = lost.then(
    () => "sent",
    (e: Error) => e.message,
  );
  vi.advanceTimersByTime(500);
  FakeSocket.all[1].close(); // the server is still down
  expect(await outcome).toBe("connection closed");

  vi.advanceTimersByTime(500);
  FakeSocket.all[2].open();
  expect(FakeSocket.all[2].sent).toEqual([]);
  expect(seen).toEqual(["open", "reconnecting", "open"]);
});
