# OpenScholar

School management suite (Rust + Axum).

## Storage / attachments

Document attachments use an S3-shaped configuration (`S3_ENDPOINT`, `S3_BUCKET`,
`S3_REGION`, `S3_ACCESS_KEY`, `S3_SECRET_KEY`, `S3_PUBLIC_BASE_URL`) but this
build stores bytes on the local filesystem under `./uploads` for both `Local` and
`S3` modes (no network calls are made). A real signed-PUT backend can drop in
later without changing the key scheme.
