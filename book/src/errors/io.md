# `config::io`

Reading a source failed. `message` is the formatted IO error. The core type does not
hold `std::io::Error`.

A missing file that was named explicitly (`{PREFIX}_CONFIG`, `LoaderBuilder::file`) is
`config::missing`, not this code. A permission or disk failure while reading a file
that was found is `config::io`.
