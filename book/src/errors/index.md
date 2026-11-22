# Error catalog

Match `ConfigError::code()` rather than the enum shape. The enum is `#[non_exhaustive]`.
Each code below is one variant. Warnings are not errors: they use `config::warn::*`
(`assumed_version`, `document_version`, `unknown_key`, `world_writable`,
`secret_world_readable`, `missing_profile`, `ignored_include`, `deprecated_key`) and
land on `LoadReport`.

`config::missing_profile` the error and `config::warn::missing_profile` the warning are
different. The error is `RequireActiveProfile`. The warning is a profile file that was
not required and was not there.

`config::migration` and `config::reload_rejected` are reserved for 0.2. The variants
exist so a caller can match the code. 0.1 does not raise them from the built-in
pipeline.
