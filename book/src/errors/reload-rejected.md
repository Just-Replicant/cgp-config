# `config::reload_rejected`

A reload changed a field marked `reload = "restart"` and was not applied. `paths` lists
those fields. The previous config stays in place.

0.1 accepts the attribute and stores it on `FieldMeta`. The reload loop that raises
this code is 0.2.
