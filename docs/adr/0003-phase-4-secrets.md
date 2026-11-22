# ADR-003: Phase 4 secrets and diagnostics

* Status: Accepted
* Date: 2026-11-22
* Phase: P4 (secrets and diagnostics)
* Design plan: [`docs/design-plan.md`](../design-plan.md) (§6.9, §6.11, §6.17, §15, §18)

## Context

P4 adds `Secret<T>`, the `env:` / `file:` / `docker:` resolvers, redaction, a fingerprint,
`explain`, and the `miette` diagnostic. The design plan's `CanResolveSecrets` walk cannot
see a `Secret<T>` field from a generic loader: autoref specialization only considers the
fallback impl inside a generic function. Resolution therefore happens while the field
deserializes. `SecretsResolverComponent` and the per-scheme `SecretResolverComponent` are
different traits, so the plan's one-line preset override is not enough on its own.

## Decision

1. **Secret bytes never enter `ConfigValue` or `Meta`.** A reference stays in the tree as
   `{ secret = "scheme:rest" }` with `Meta::secret`. A literal stays in the tree, is
   marked secret, and is redacted wherever the tree is shown. Resolved bytes live only in
   `Secret<T>` or `LazySecret<T>`, and are zeroized when the last `Arc` drops.
2. **Resolution is a thread-local session installed before deserialize.** `LoadLayered`
   (the `alloc` impl) calls `CanOpenSecretSession` and holds the guard across
   deserialize. `CoreConfigPreset` wires `NoSecretSession`, which denies every reference.
   `SecretsPreset` overrides that with `OpenStdSession`. The guard is `std`-only; without
   `std` it is a no-op and every reference is `SecretsDenied`. There is no `unsafe`. The
   session is saved and restored, so nested loads do not leak a resolver.
3. **`CanOpenSecretSession` is a new core component.** Hand-written tables that do not use
   `CoreConfigPreset` add `SecretSessionComponent: NoSecretSession`. The `alloc`
   `LoadLayered` impl always requires the component. This is a source break, of the same
   kind as ADR-002's reporter changes.
4. **`DenySecrets` stays the preset default and is a documented no-op.** Denial happens
   while `Secret<T>` deserializes under the deny session (`ConfigError::SecretsDenied`).
   A load with no secret fields, or with only literal secrets, succeeds. `LazySecret`
   does not fail the load: it resolves on first `expose_secret`.
5. **`SecretsPreset` wires both traits.** It extends `TwelveFactorPreset` and sets
   `SecretSessionComponent: OpenStdSession` (so load resolves), `SecretResolverComponent:
   UseDelegate<StdSecretSchemes>` (so a direct `CanResolveSecret` call resolves), and
   `SecretRootsComponent: StdSecretRoots`. `StdLoader` stays on `TwelveFactorPreset`.
   Callers opt in on their own context.
6. **Mock secrets are opt-in.** `MockSecretsPreset` extends `TestConfigPreset` with
   `OpenMockSession` and reads a `secret_map` field (`SecretMap`, whose `Debug` lists
   names only). `TestConfigPreset` and `MemoryLoader` are unchanged.
7. **Schemes share one implementation.** The CGP providers and the session reader call
   the same functions (`resolve_env`, `resolve_file`, `resolve_docker`, `read_within`).
   The reader owns an `EnvSnapshot`, `SecretRoots`, and `max_bytes`. `env:` reads only the
   snapshot. `file:` must sit under a secret root after canonicalize (symlink included).
   `docker:` is one path segment under `/run/secrets/{name}`. Unknown schemes (`vault:`,
   `aws:`, `gcp:`) are left for 0.3. The async twin is a trait sketch; 0.1 ships sync
   schemes only. Delegate tables are `#[unchecked]` because the generated check asks the
   table itself to be a context, and the provider bounds do not hold there.
8. **Roots, in order.** The config-file directory (an empty directory is `.`), then
   `CREDENTIALS_DIRECTORY` from the env snapshot, then `/run/secrets` when it is a
   directory. The first matching root wins. An escape is `ConfigError::SecretDenied`.
   File bytes are not trimmed. Reads honour `LoadLimits::max_bytes`. IO errors use a
   fixed message and a redacted origin, never the secret bytes or the OS path text.
9. **Serde cannot name `D::Error`.** The session reports through `de::Error::custom`
   with a NUL-prefixed sentinel. The value deserializer maps that sentinel back to
   `ConfigError` before the secret path would replace the message with `invalid secret
   value`.
10. **Crate split.** `cgp-config-secrets` depends on core and the fork, not on providers.
    Providers' `secrets` feature depends on `cgp-config-secrets`. `SecretRoots::discover`
    lives in the secrets crate; `StdSecretRoots` and `OpenStdSession` live in providers,
    because they see `ConfigLocation`. Presets live in `presets.rs`: `cgp_preset!` looks
    up the parent preset in the same module.
11. **`#[config(secret)]` or a `Secret` / `LazySecret` type.** The derive sets
    `FieldMeta.secret` when the attribute is present or when the type's last segment,
    after unwrapping `Option`, `Vec`, `Box`, `Arc`, or `Rc`, is `Secret` or `LazySecret`.
12. **Redaction and the fingerprint.** `LoadReport::fingerprint` is FNV-1a 64 of the
    canonical redacted JSON (secret nodes become `"[REDACTED]"`, map keys sorted). It is
    not `std::DefaultHasher`. `explain` shows a redacted reference only for a
    `{ secret = "scheme:rest" }` table. A plain string is never shown, even when it looks
    like `env:...`. `ConfigLoaded` carries the hex fingerprint and the winning origin.
    The stamp is stored on `LoadReport` because core cannot hold `ConfigValue`.
    `CanReportWarning::note_loaded` is how the deserializer records it, including on the
    error path, so `IgnoreWarnings` still compiles. Adding `note_loaded` is a source
    break for external reporters.
13. **Permission warning.** On Unix, a file whose mode includes group or other read
    (`mode & 0o044`) warns `LoadWarning::SecretWorldReadable` when it contains a secret
    reference or a schema secret field with a non-null value. Reference sniffing from
    ADR-002 item 15 stays; schema detection is added beside it.
14. **`miette`.** `ConfigDiagnostic` wraps `ConfigError`. Help text names the env var and
    the file key. A secret span has no snippet: the source is blanked with spaces so
    other offsets stay valid. An `anyhow` context error drops labels, because `anyhow`
    does not implement `Diagnostic`.
15. **`from_toml_str` and `from_json_str` stay on the deny session.** Literals in
    `Secret<String>` work. A reference table is `SecretsDenied`. `from_toml_str_with`
    still takes the caller callback and opens the session before deserialize, so an outer
    resolving session cannot leak in.

## Consequences

* A context that loads `Secret<T>` references uses `SecretsPreset` (or its own
  `OpenStdSession`) and holds `env_snapshot`, `config_location`, `load_report`, and the
  other twelve-factor fields. `config_check!` sees the new components through the preset.
* External `CanReportWarning` impls and hand-written `LoadLayered` tables from before
  this phase need `note_loaded` and `SecretSessionComponent`.
* Reload diffs (P6) are out of scope. The P4 acceptance is the redacted dump, `explain`,
  and `Debug` / `Display` / the load log.
* `vault:`, `aws:`, and `gcp:` are not resolved in 0.1.
