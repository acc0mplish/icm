# Korean recall for SQLite

The `korean` build feature improves Korean queries in the v2 memory recall
engine. It works without embeddings and also applies to the local prompt hook
and MCP/HTTP recall, which use the same v2 SQLite search.

In the `acc0mplish/icm` fork, the `icm-cli` default features include `korean`:

```sh
cargo build --release --locked -p icm-cli
```

Builds using `--no-default-features` must explicitly include `korean` in their
feature list. The `icm-store` and `icm-mcp` library features remain opt-in.
The feature embeds Lindera 6.2 and `ko-dic`, increasing build time, binary size
and per-process cold-start cost on the first Korean query. Building the
dictionary may require network access;
the resulting binary does not download a dictionary at runtime. Lindera is
MIT licensed and the underlying mecab-ko-dic is Apache-2.0 licensed.
`icm config` reports when Korean query analysis is compiled into the binary.
Official upstream installers and `icm upgrade` select upstream binaries, which
do not include this fork's feature unless their release also enables it. Some
release build configurations explicitly disable default features; they must
add `korean` to include it.

The original FTS index and its existing text stay unchanged. For queries
containing Hangul, Lindera finds Korean content surfaces, then v2 searches for
their prefixes in the original FTS index and for literal substrings in memory
summaries, topics and keywords. The substring candidates are ranked separately
and fused with the FTS candidates before vector/date fusion. A substring-only
hit has no invented BM25 score. NFC and NFD Hangul are both considered.

This approach works with existing databases and older ICM binaries without a
schema migration or reindexing. It is not a document-side morphological index:
it may miss irregular inflections and can match a surface inside an unrelated
word. The substring search scans the memory table, so latency grows with the
number and size of memories; measure it on your corpus before enabling this on
a large deployment. Analysis is bounded to the first 4096 bytes and 12 Korean
terms. On a dictionary or analysis error, v2 falls back to ordinary recall.

The legacy recall engine and the transcript, feedback and memoir FTS indexes
are unchanged. Postgres and OpenSearch backends are also unchanged. Technical
identifiers and non-Korean queries continue to use the existing FTS behavior.
