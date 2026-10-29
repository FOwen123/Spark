const KEY = "spark-token";

/** Reads the server token from a `#token=...` URL fragment (kept out of server logs) or session storage. */
export function tokenFromHash(hash: string): string | null {
  return new URLSearchParams(hash.replace(/^#/, "")).get("token");
}

export function sessionToken(): string {
  const fromHash = tokenFromHash(location.hash);
  if (fromHash) {
    sessionStorage.setItem(KEY, fromHash);
    history.replaceState(null, "", location.pathname + location.search);
    return fromHash;
  }
  return sessionStorage.getItem(KEY) ?? "";
}
