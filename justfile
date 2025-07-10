# Shebang to run with `just` and pass the rest of the file to it.
#!/usr/bin/env just --justfile

# Allow passing arbitrary arguments to cargo-nextest
# e.g. `just test -- --nocapture`
test *ARGS:
    @cargo nextest run --all-features {{ARGS}}