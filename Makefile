RELEASE ?= 0

ifeq ($(RELEASE),1)
	PROFILE ?= release
	CARGO_ARGS = --release
else
	PROFILE ?= debug
	CARGO_ARGS =
endif

.PHONY: all
all:
	cargo build ${CARGO_ARGS}

.PHONY: install
install:
	install -D -t ${DESTDIR}/usr/libexec target/${PROFILE}/zincati
	install -D -m 644 -t ${DESTDIR}/usr/lib/zincati/config.d dist/config.d/*.toml
	install -D -m 644 -t ${DESTDIR}/usr/lib/systemd/system dist/systemd/system/*.{service,timer}
	install -D -m 644 -t ${DESTDIR}/usr/lib/sysusers.d dist/sysusers.d/*.conf
	install -D -m 644 -t ${DESTDIR}/usr/lib/tmpfiles.d dist/tmpfiles.d/*.conf
	install -D -m 644 -t ${DESTDIR}/usr/share/polkit-1/rules.d dist/polkit-1/rules.d/*.rules

.PHONY: check
check:
	cargo test
