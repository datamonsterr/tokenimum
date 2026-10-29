# ADR 0036: Web dashboard security model

Date: 2026-10-29. Status: accepted (epic E8.2).

## Decisions

1. **Loopback only.** The bind address comes from `ui.web.bind` (default `127.0.0.1:7878`). Any IP
   other than `127.0.0.1` is refused before a socket is opened: the server logs one `errors.log`
   line and does not start. Port `0` is allowed (tests).
2. **Token.** 32 bytes read from `/dev/urandom` at daemon start, hex encoded. No new crate: std
   file I/O is enough on Linux and macOS, and the read fails closed (no server) on error. The
   token lives only in memory and in `tkmd-web.json` (mode 600).
3. **Token transport.** `tkm dashboard` opens `http://127.0.0.1:<port>/#token=<hex>`. The fragment
   never reaches the server or the logs. The page script reads it and sends
   `Authorization: Bearer <hex>` on every API request. Every `/api/*` route answers 401 without a
   valid token. The comparison is constant time. `/` and `/assets/*` are static and carry no data.
4. **Host check.** Every request must have a `Host` header of `127.0.0.1:<port>` or
   `localhost:<port>`; any other value gets 421. This blocks DNS rebinding.
5. **Confirmed actions.** There are three actions: forget a fact, restore a pruned tool, and
   switch the repository preset. Each is two requests. `POST /api/actions/<kind>` validates the
   input and returns `{confirm_id, summary}`, with no change. `POST /api/confirm/<confirm_id>`
   executes. Each confirm id is single use and expires after 60 s. The page shows the summary in
   a confirmation dialog before it sends the second request. All other routes are `GET` and
   read-only.
6. **No external requests.** HTML, CSS and JS are embedded with `include_str!`, and charts are
   inline SVG drawn by the page script. A test scans the embedded assets for `http://`,
   `https://` and protocol-relative `//host` URLs. Responses carry
   `Content-Security-Policy: default-src 'self'; frame-ancestors 'none'`.

7. **Action resource limits.** Action JSON is limited to 16 KiB, each input field to 4 KiB,
   and outstanding confirmations to 32. Expired confirmations are removed on state, prepare,
   and confirm requests. Presets must be one of the config presets, and restore checkouts must
   be absolute and recorded in the deny-list table.

8. **Shutdown.** The server rejects actions once closing starts and waits for any executing
   action before returning. Graceful connection shutdown is bounded; a stalled body cannot
   prepare or execute an action after shutdown.
