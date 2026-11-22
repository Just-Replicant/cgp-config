# Pipeline

Every built-in load runs five stages:

1. **Collect.** For each source in the preset's list: load, prepare the layer
   (`$version`, `$profiles`, canonicalize names), merge onto the accumulator. Merge of
   two different shapes is `config::merge_conflict`.
2. **Interpolate.** Off unless the field is `#[config(interpolate)]` and the context
   uses `InterpolatingPreset`. Other fields keep `${` as text.
3. **Deserialize.** Serde, with provenance. A type mismatch is `config::type`. Every
   missing required field is reported together.
4. **Resolve secrets.** The session installed for this load. The default session denies
   references (`config::secrets_denied`).
5. **Validate.** `ConfigValidatorComponent`. `Nested` runs each nested section's
   validator and aggregates the errors with their paths.

`LoadReport` collects warnings, the profile tables that were selected, and a fingerprint
(FNV-1a 64 of the canonical redacted JSON). `ConfigLoaded` logs the winning origin and
the fingerprint, never values.
