# Provenance

These five pages are copied verbatim from the Go platform
(`flowcatalyst-go/docs/published/*.md` at `373fe93`, repository HEAD
`73a6918`), where they are embedded and served as the platform
documentation (`GET /api/docs`, `GET /api/docs/platform/{slug}`).

The Rust platform embeds the same files (`crates/fc-platform/src/app_docs/platform_docs.rs`,
`include_str!`) so the routes answer as Go's do: the slug is the file name
without `.md` and its `NN-` ordering prefix, the title the first `# ` heading.
This file is not one of them. Edit the pages in step with Go until Rust is
their only home.
