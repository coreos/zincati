---
layout: default
nav_order: 1
parent: Development
---

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

If you prefer running builds in a containerized environment, you can use the FCOS buildroot image at `quay.io/coreos-assembler/fcos-buildroot:testing-devel`:

```sh
docker pull quay.io/coreos-assembler/fcos-buildroot:testing-devel
docker run --rm -v "$(pwd):/source:z" quay.io/coreos-assembler/fcos-buildroot:testing-devel bash -c "cd source; make"
```

The FCOS buildroot image is the same image that is used by integration jobs in CI.
It contains all the required dependencies and can be used to build other CoreOS projects too (not only Zincati).

## Assemble custom OS images

`coreos-assembler` ([`cosa`](https://github.com/coreos/coreos-assembler)) makes it very handy to embed build artifacts in a custom OS image, in order to test patches in the final environment.

Once a new `cosa` workspace has been initialized, you can place the binaries in the `overrides/` directory before building your custom image:

```sh
pushd /tmp
mkdir test-image
cd test-image
cosa init https://github.com/coreos/fedora-coreos-config
popd
docker run --rm -v "$(pwd):/source:z" -v "/tmp/test-image:/assembler:z" \
    -e DESTDIR="/assembler/overrides/rootfs" -e TARGETDIR="/assembler/tmp/zincati/target" \
    quay.io/coreos-assembler/fcos-buildroot:testing-devel bash -c "cd source; make install"
pushd /tmp/test-image
cosa fetch
cosa build
```

For more details, see `coreos-assembler` [overrides documentation](https://coreos.github.io/coreos-assembler/working/#using-overrides).

### `build-fast` for faster iteration

It is possible to use the CoreOS Assembler's [`build-fast`][build-fast-cmd] command for faster iteration.
See [here][build-fast-instructions] for instructions on fast-building a qemu image for testing.

[build-fast-cmd]: https://github.com/coreos/coreos-assembler/blob/master/src/cmd-build-fast
[build-fast-instructions]: https://github.com/coreos/coreos-assembler/blob/2f834d37353ca5f40b460eae2aea73ef995bc710/docs/kola/external-tests.md#fast-build-and-iteration-on-your-projects-tests
