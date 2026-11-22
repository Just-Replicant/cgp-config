# cgp-config

`cgp-config` loads layered, typed configuration. A context (a plain struct) chooses, at
compile time, where values come from, how layers merge, how they deserialize, and how
the result is validated. Each stage is a CGP component.

This book covers the 0.1 surface (phases P1–P5). Reload, schema migrations, and
templates arrive in 0.2. The crate version is `0.1.0-alpha.1`.

Runnable copies of the snippets live as crate doctests and in
`examples/multi-crate-workspace`. `cargo xtask book` builds this book when `mdbook` is
installed. That step is not part of `cargo xtask ci`.

Decisions that changed the design plan are in `docs/adr/`, in particular ADR-004 for
getters, `ProjectSection`, `quick::load`, and partial configs.
