# `config::duplicate_key`

A key appears twice in one document or one layer. The parsers reject it. The schema
pass also raises it when one layer sets a field under two names (the key and an alias,
or a mount and the serde key).

`meta` is the second occurrence. `first` is the first, when the pass can point at it.
