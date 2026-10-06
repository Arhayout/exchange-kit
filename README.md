# exchange-kit

Rust primitives for exchange connectivity: request signing, local order-book reconstruction, and concurrent shared state.

## Modules

| Module | Purpose |
|--------|---------|
| [`signatures`](src/signatures.rs) | Huobi-style HMAC-SHA256 REST request signing (RFC 3986 query encoding) |
| [`orderbook`](src/orderbook.rs) | Replay snapshot / book / trade JSON streams into a local top-of-book |
| [`shared_data`](src/shared_data.rs) | Thread-safe mailboxes: latest-value (`AtomicU128`) and ordered FIFO |

Signing follows the [Huobi API signature scheme](https://www.huobi.com/en-us/opend/newApiPages/?id=419); see [`src/huobi_signature.md`](src/huobi_signature.md).

## Quick start

Requires the pinned nightly toolchain in [`rust-toolchain.toml`](rust-toolchain.toml) (needed for `cargo bench`).

```bash
cargo test
cargo bench
```

## Design notes

**Signatures** — Canonical query strings use uppercase percent-encoding (`:` → `%3A`). The pre-sign payload is `METHOD\\nhost\\npath\\nquery`.

**Order book** — Levels live in `BTreeMap`s. Snapshots replace a side; book updates upsert / remove by quantity; trades retire crossed levels then subtract size at the print price. Malformed frames are skipped.

**Shared state**
- `LatestSharedData` — single-slot consume-on-read mailbox packed into one `AtomicU128` (`{tag, f64 bits}`), so every NaN payload round-trips.
- `OrderedSharedData` — FIFO via `Mutex<Vec + head cursor>`, reclaiming storage after a full drain.

Reference timings (Apple Silicon, pinned nightly) are in [`BENCH_RESULTS.txt`](BENCH_RESULTS.txt).

## License

MIT
