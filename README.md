mdbook preprocessor that copies files into the book source directory.

The use case is for linking to files that are generated as an output of another process.

Example preprocessor configuration:

```toml
[preprocessor.file-search]
files = [
    {alias = "foo", path = "../outputs/foo.txt"}
]
```
