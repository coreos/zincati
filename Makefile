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
	cargo build "--target-dir=${TARGETDIR}" ${CARGO_ARGS}

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

.PHONY: check
check:
	cargo test "--target-dir=${TARGETDIR}"
