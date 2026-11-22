# `config::parse`

The source text is not valid in its format. The origin names the file or the env
document. `span` is set when the parser reports one (`file:line:column` in `Display`).

TOML and JSON both reject a duplicate key in one document as `config::duplicate_key`
rather than a parse error. A `{PREFIX}_DOCUMENT` that is not JSON is a parse error.
