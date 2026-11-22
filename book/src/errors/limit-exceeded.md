# `config::limit_exceeded`

A `LoadLimits` bound was exceeded while parsing or walking a document. `what` is the
field name (`max_depth`, `max_keys`, `max_bytes`, and the other limits). `limit` is the
configured value.

The parsers enforce the limits as they read, so a hostile document fails before the
tree is complete.
