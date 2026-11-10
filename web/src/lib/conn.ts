// The single UI transport: one WebSocket to the Spark server carrying requests (with reply ids)
// down and snapshots, patches and other server events back.

export type ServerMsg = { t: string; [k: string]: unknown };
type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void };
/** `connecting` until the first socket opens or fails; a socket that closes leaves `reconnecting`. */
export type ConnStatus = "connecting" | "open" | "reconnecting";

export class Conn {
  private ws: WebSocket | null = null;
  private rid = 0;
  private pending = new Map<number, Pending>();
  private queue: string[] = [];
  private listeners = new Set<(m: ServerMsg) => void>();
  private statusListeners = new Set<(s: ConnStatus) => void>();
  status: ConnStatus = "connecting";

  constructor(private url: string) {
    this.connect();
  }

  private connect() {
    const ws = new WebSocket(this.url);
    this.ws = ws;
    ws.onopen = () => {
      for (const m of this.queue.splice(0)) ws.send(m);
      this.setStatus("open");
    };
    ws.onmessage = (e) => {
      const msg = JSON.parse(e.data as string) as ServerMsg;
      if (msg.t === "reply") {
        const p = this.pending.get(msg.rid as number);
        this.pending.delete(msg.rid as number);
        if (msg.ok) p?.resolve(msg.result);
        else p?.reject(new Error(String(msg.error)));
        return;
      }
      for (const l of this.listeners) l(msg);
    };
    ws.onclose = () => {
      // Whatever was waiting has failed, and so has whatever was queued: a caller told "connection
      // closed" must not see its message sent on the next socket.
      for (const p of this.pending.values()) p.reject(new Error("connection closed"));
      this.pending.clear();
      this.queue = [];
      this.setStatus("reconnecting");
      setTimeout(() => this.connect(), 500);
    };
  }

  private setStatus(s: ConnStatus) {
    if (s === this.status) return;
    this.status = s;
    for (const l of this.statusListeners) l(s);
  }

  send(msg: Record<string, unknown>) {
    const text = JSON.stringify(msg);
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(text);
    else this.queue.push(text);
  }

  request<T = unknown>(msg: Record<string, unknown>): Promise<T> {
    const rid = ++this.rid;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(rid, { resolve: resolve as (v: unknown) => void, reject });
      this.send({ ...msg, rid });
    });
  }

  on(fn: (m: ServerMsg) => void) {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  }

  onStatus(fn: (status: ConnStatus) => void) {
    this.statusListeners.add(fn);
    return () => {
      this.statusListeners.delete(fn);
    };
  }
}
