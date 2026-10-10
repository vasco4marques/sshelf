# sshelf

Pick a host from `~/.ssh/config` with the arrow keys and connect to it.

## Install

Requires the Rust toolchain to build and `ssh` on your `PATH`.

```sh
cargo install --path .
```

## Usage

```sh
sshelf                    # pick a host, Enter connects (type to filter, Esc to quit)
sshelf --configurations   # Add, Edit or Delete hosts in ~/.ssh/config
```

- **Add**: asks for the alias, HostName, User, Port, an IdentityFile from `~/.ssh` (keys with a matching `.pub`), and any extra options. Empty answers are skipped.
- **Edit**: lists the host's options. Select one to change it (an empty value removes it), use **Add new field** for new options, then **Save**.
- **Delete**: removes the selected alias, or the whole block if it is the block's only alias.

## Notes

- Only concrete aliases are listed. Wildcard patterns (`*`, `?`, `!`) are hidden.
- `Include` directives are ignored. Only `~/.ssh/config` itself is read and edited.
- Changes are written to a temp file, validated with `ssh -G`, then renamed over the config. If anything fails, your config is left untouched.
