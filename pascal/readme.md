# Pascal Interpreter on RUST

## Run tests:

```
cargo install cargo-tarpaulin
```

```
cargo tarpaulin --out Html --output-dir coverage --exclude-files "src/ast.rs" --exclude-files "src/main.rs" --exclude-files "src/token.rs"
```

## Run program:

```
cargo run
```