# `config::merge_conflict`

Two layers disagree on the shape of a node: a map over a scalar, an array over a map.
The lower layer is not replaced. `base` and `overlay` are the origins. `base_kind` and
`overlay_kind` are `map`, `array`, `string`, and the other value kinds.

Deep merge of two maps, and a scalar overlay that replaces a scalar, are not conflicts.
