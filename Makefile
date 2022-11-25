RELEASE ?= 0
TARGETDIR ?= target

ifeq ($(RELEASE),1)
	PROFILE ?= release
	CARGO_ARGS = --release
else
	PROFILE ?= debug
	CARGO_ARGS =
endif

.PHONY: all
all: build check

.PHONY: build
build:
	cargo build "--target-dir=${TARGETDIR}" ${CARGO_ARGS} --features drogue

.PHONY: install
install: build
	install -D -t ${DESTDIR}/usr/libexec "${TARGETDIR}/${PROFILE}/zincati"
	install -D -m 644 -t ${DESTDIR}/usr/lib/zincati/config.d dist/config.d/*.toml
	install -D -m 644 -t ${DESTDIR}/usr/lib/systemd/system dist/systemd/system/*.service
	install -D -m 644 -t ${DESTDIR}/usr/lib/sysusers.d dist/sysusers.d/*.conf
	install -D -m 644 -t ${DESTDIR}/usr/lib/tmpfiles.d dist/tmpfiles.d/*.conf
	install -D -m 644 -t ${DESTDIR}/usr/share/polkit-1/rules.d dist/polkit-1/rules.d/*.rules
	install -D -m 644 -t ${DESTDIR}/usr/share/polkit-1/actions dist/polkit-1/actions/*.policy
	install -D -m 644 -t ${DESTDIR}/usr/share/dbus-1/system.d dist/dbus-1/system.d/*.conf


.PHONY: install-fast
install-fast: build
	install -D -t test-image/overrides/rootfs/usr/libexec "${TARGETDIR}/${PROFILE}/zincati"
	install -D -m 0644 -t test-image/overrides/rootfs/etc/zincati/config.d tests/fixtures/99-drogue-iot.toml

.PHONY: image
image: install-fast
	cd test-image && cosa build

.PHONY: image-fast
image-fast: install-fast

	cd test-image && cosa build-fast

.PHONY: check
check:
	cargo test "--target-dir=${TARGETDIR}"
