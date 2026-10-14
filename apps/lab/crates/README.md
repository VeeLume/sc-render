# crates/

Library crates, picked up by the root workspace's `members = ["crates/*"]`.

The split that has held up across projects:

- **`svc-*`** — services the shell owns: one owner per external resource (a file
  watcher, a parser cache, an HTTP client). Anything that touches the
  filesystem, a device, or the network lives here, once.
- **`*-kit`** — leaf utilities with no app knowledge (paths, atomic IO, small
  domain types). Everything else may depend on these.

Rule of thumb: a crate exists to enforce a dependency boundary or to be tested
in isolation. Splitting for tidiness alone just adds `Cargo.toml`s.
