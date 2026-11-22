# `config::migration`

A migration step failed. `from` and `to` are the versions. `message` says why.

0.1 records `$version` and does not run migrations. This code is part of the enum so
callers can match it. The built-in pipeline does not raise it until 0.2.
