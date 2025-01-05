# moshi-android

Moshi example for Android.

```
cargo install cargo-apk
sh build.sh
```

## Potential issues

* the following doesn't seem to be necessary as of 2025-01-05 *

Use `cargo-apk` to build and run. Requires a patch to workaround [an upstream bug](https://github.com/rust-mobile/cargo-subcommand/issues/29).

One-time setup:

```sh
cargo install \
    --git https://github.com/parasyte/cargo-apk.git \
    --rev 282639508eeed7d73f2e1eaeea042da2716436d5 \
    cargo-apk
```

Build and run:

```sh
cargo apk build -r
```
