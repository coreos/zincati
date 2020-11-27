# Development quickstart

This is quick start guide for developing and building this project from source on a Linux machine.

## Get the source

The canonical development location of this project is on GitHub. You can fetch the full source with history via `git`:

```sh
git clone https://github.com/coreos/zincati.git
cd zincati
```

It is recommend to fork a copy of the project to your own GitHub account, and add it as an additional remote:

```sh
git remote add my-fork git@github.com:<YOURUSER>/zincati.git
```

## Install Rust toolchain

This project is written in Rust, and requires a stable toolchain to build. Additionally, `clippy` and `rustfmt` are used by CI jobs to ensure that patches are properly formatted and linted.

You can obtain a Rust toolchain via many distribution methods, but the simplest way is via [rustup](https://rustup.rs/):

```sh
rustup component add clippy
rustup component add rustfmt
rustup install stable
```

## Build and test

Building and testing is handled via `cargo` and `make`:

```sh
make build
make check
```

If you prefer running builds in a containerized environment, you can use the provided `Dockerfile`:

```sh
docker build -f contrib/Dockerfile.dev -t coreos/zincati:dev .
docker run --rm -v "$(pwd):/source" -v "/tmp/assembler:/assembler" coreos/zincati:dev
```

## Assemble custom OS images

`coreos-assembler` ([`cosa`](https://github.com/coreos/coreos-assembler)) makes it very handy to embed build artifacts in a custom OS image, in order to test patches in the final environment.

Once a new `cosa` workspace has been initialized, you can place the binaries in the `overrides/` directory before building your custom image:

```sh
pushd /tmp
mkdir test-image
cd test-image
cosa init https://github.com/coreos/fedora-coreos-config
popd
docker build -f contrib/Dockerfile.dev -t coreos/zincati:dev .
docker run --rm -v "$(pwd):/source" -v "/tmp/test-image:/assembler" coreos/zincati:dev
pushd /tmp/test-image
cosa fetch
cosa build
```
