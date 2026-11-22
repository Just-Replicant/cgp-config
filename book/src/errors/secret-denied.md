# `config::secret_denied`

A secret reference points outside the allowlisted roots or names a scheme the session
does not implement. `reference` is redacted. `file:` and `docker:` must stay under
`StdSecretRoots` (`CREDENTIALS_DIRECTORY`, the config directory, `/run/secrets`).
