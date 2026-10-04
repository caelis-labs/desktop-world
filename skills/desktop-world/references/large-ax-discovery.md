# Large browser AX discovery

An empty `objects` array with `coverage.complete:false` is not absence. Read
`coverage.unavailable_sources`, `visited_nodes` and `continuation`. Native
continuation first drains result pages, then resumes the retained traversal
queue. The visited count is cumulative and may stay unchanged on result pages.
Keep the same scope, projection, fields, match and traversal budget when
following it. The helper may make a tiny `max_output_bytes` adjustment for
response-envelope growth. Cursors expire with the helper or after 90 seconds.

Recommended order:

1. Use the current `seat.focused_object` / `seat.foreground_window` Ref.
2. Read the focused Ref and its `parent` chain with detail observations.
3. Use a window/document-scoped `match` with role and exact or contained name.
4. If a separately authorized browser API exists, prefer a browser role/name
   locator such as CDP `queryAXTree` or Playwright `getByRole` for huge pages.
5. Use screenshot plus anchor only as an explicit last resort.

For step 3, request `fields:['role','name']`, `max_results:8`,
`max_text_runes:64`, `max_output_bytes:4096`, a finite visited allowance and
at most a 10-second deadline. Set a caller budget such as eight scan calls
and 32 KiB total visible text; report the bounded partial result if that
budget ends. The helper also stops a broad multi-call scan at its cumulative
output cap with `ax_output_limit`. `ax_scan_limit` means the retained native
key/visit cap was reached. Neither code establishes absence.

Avoid a whole-page role-only link enumeration: each page is small, but a long
sequence consumes context. `getFullAXTree` and unbounded ARIA snapshots are
diagnostics, not model-facing output. The
[fragmented-text recipe](fragmented-text.md) applies after a document Ref is
already known; it solves text fragmentation rather than target discovery.

Resumed AX scans sample a live tree, not a frozen snapshot. The helper keeps
coverage incomplete on resume. Re-read a found Ref before acting, and keep
the original turn/grant. No continuation renews authority.
