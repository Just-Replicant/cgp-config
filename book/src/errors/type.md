# `config::type`

A value does not have the type the struct expects: a string where a number is required,
a number where a duration needs a unit (`30` instead of `"30s"`), a table where a
scalar is required. `path` is the key. On a secret path the message is fixed text and
does not echo the value.
