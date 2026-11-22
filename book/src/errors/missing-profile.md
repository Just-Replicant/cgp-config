# `config::missing_profile`

The active profile is not defined, and the loader requires it (`RequireActiveProfile`,
which `StrictPreset` wires). The profile name is the one bootstrap selected (`default`
unless `{PREFIX}_PROFILE` or `profile()` set another).

A profile file that is optional and absent is `config::warn::missing_profile`, and the
load continues.
