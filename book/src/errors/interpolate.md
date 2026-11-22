# `config::interpolate`

Interpolating an opted-in string failed: a missing `${name}`, a bad substitution, or a
value that is not a string. The field must be `#[config(interpolate)]` and the context
must use `InterpolatingPreset`. Fields that did not opt in keep `${` as written and do
not raise this error.

The interpolator does not read files and does not resolve secrets.
