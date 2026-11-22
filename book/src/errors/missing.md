# `config::missing`

A required field or a required source is absent. Every missing field of the struct is
reported together. `expected` is a type name or a description, and may be empty.

An explicit config path that does not exist uses this code. A search that finds no file
does not, unless `require_file` is set: the load continues from defaults and env.
