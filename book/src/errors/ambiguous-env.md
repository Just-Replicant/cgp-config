# `config::ambiguous_env`

One environment variable maps to more than one config path. Env segments fold to
`snake_case`, so `HTTP__PORT` and a field whose `env` is `PORT` under mount `http` can
name the same variable as another field. `candidates` lists every path.

The derive rejects the same clash inside one struct (duplicate `env_raw`, or two keys
that fold equal) before this error can happen at runtime. The runtime error is for a
root assembled from sections where the clash is not visible to one derive.
